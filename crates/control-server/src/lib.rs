use std::{convert::Infallible, path::PathBuf, time::Duration};

use anyhow::Result;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode, Uri},
    middleware::{self, Next},
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use domain::{
    AgentRole, ApprovalGate, Artifact, EventEnvelope, EventScope, ExecutorProfile, GoalSpec,
    NewAgentRole, NewExecutorProfile, NewGoalSpec, NewProject, NewRun, NewSkillDefinition,
    NewWorkflowTemplate, PairingSession, Project, Run, SkillDefinition, WorkflowTemplate,
};
use executors::{default_health_checks, ExecutorHealth};
use futures_util::StreamExt;
use observability::EventBus;
use orchestrator::{CompiledGoal, RunOrchestrator, RunStateSnapshot, SuperOwner};
use persistence::OrchestratorStore;
use runtime::{ProjectContextService, ProjectContextSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};

#[derive(Clone)]
pub struct AppState {
    pub store: OrchestratorStore,
    pub events: EventBus,
    pub frontend_dist: Option<PathBuf>,
}

impl AppState {
    pub fn in_memory() -> Result<Self> {
        Ok(Self {
            store: OrchestratorStore::open_in_memory()?,
            events: EventBus::default(),
            frontend_dist: None,
        })
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Deserialize)]
struct ApprovalResolutionRequest {
    resolved_by: Option<String>,
    notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PairingSessionCreateRequest {
    label: Option<String>,
    expires_in_minutes: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PairingSessionResponse {
    session: PairingSession,
    remote_url: String,
}

#[derive(Debug, Deserialize)]
struct NewSkillBindingRequest {
    skill_id: String,
}

#[derive(Debug, Deserialize)]
struct CompileGoalRequest {
    agents_md_override: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProjectContextResponse {
    snapshot: ProjectContextSnapshot,
}

#[derive(Debug)]
pub struct ApiError(anyhow::Error);

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: self.0.to_string(),
            }),
        )
            .into_response()
    }
}

pub fn app(state: AppState) -> Router {
    let frontend_dist = state.frontend_dist.clone();
    let api_router = Router::new()
        .route("/health", get(health))
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/{project_id}/context", get(get_project_context))
        .route("/api/executors", get(list_executors).post(create_executor))
        .route("/api/executors/health", get(list_executor_health))
        .route("/api/roles", get(list_roles).post(create_role))
        .route("/api/roles/{role_id}/skills", post(bind_skill_to_role))
        .route("/api/skills", get(list_skills).post(create_skill))
        .route("/api/goals", get(list_goals).post(create_goal))
        .route("/api/goals/{goal_id}/compile", post(compile_goal))
        .route("/api/workflows", get(list_workflows).post(create_workflow))
        .route("/api/runs", get(list_runs).post(create_run))
        .route("/api/runs/{run_id}", get(get_run))
        .route("/api/runs/{run_id}/artifacts", get(list_run_artifacts))
        .route(
            "/api/runs/{run_id}/steps/{run_step_id}/complete",
            post(complete_run_step),
        )
        .route("/api/approvals", get(list_approvals))
        .route("/api/approvals/{approval_id}/approve", post(approve_gate))
        .route("/api/approvals/{approval_id}/reject", post(reject_gate))
        .route(
            "/api/pairing-sessions",
            get(list_pairing_sessions).post(create_pairing_session),
        )
        .route(
            "/api/pairing-sessions/{pairing_id}/revoke",
            post(revoke_pairing_session),
        )
        .route("/api/events", get(list_events))
        .route("/api/events/stream", get(stream_events))
        .route("/api/events/test", post(publish_test_event))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            authorize_remote_api,
        ))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

    let router = if let Some(frontend_dist) = frontend_dist {
        api_router
            .route("/", get(serve_frontend_index))
            .route("/index.html", get(serve_frontend_index))
            .nest_service("/assets", ServeDir::new(frontend_dist.join("assets")))
    } else {
        api_router
    };

    router.with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn serve_frontend_index(State(state): State<AppState>) -> Result<Html<String>, ApiError> {
    let frontend_dist = state
        .frontend_dist
        .clone()
        .ok_or_else(|| anyhow::anyhow!("frontend dist path is not configured"))?;
    let index_path = frontend_dist.join("index.html");
    let html = std::fs::read_to_string(&index_path).map_err(anyhow::Error::from)?;
    Ok(Html(html))
}

async fn list_projects(State(state): State<AppState>) -> Result<Json<Vec<Project>>, ApiError> {
    Ok(Json(state.store.list_projects()?))
}

async fn create_project(
    State(state): State<AppState>,
    Json(input): Json<NewProject>,
) -> Result<(StatusCode, Json<Project>), ApiError> {
    let mut project = state.store.create_project(input)?;
    if std::path::Path::new(&project.workspace_path).exists() {
        if let Ok(snapshot) = ProjectContextService::default().discover(&project.workspace_path) {
            project = state.store.update_project_agents_md(
                project.id,
                snapshot.root_agents_md.as_ref().map(|doc| doc.path.clone()),
                Some(snapshot.discovered_at),
            )?;
        }
    }
    emit_event(
        &state,
        EventEnvelope::new(
            EventScope::Project,
            "project.created",
            format!("Created project {}", project.name),
            json!({"project_id": project.id}),
        ),
    )?;
    Ok((StatusCode::CREATED, Json(project)))
}

async fn get_project_context(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Json<ProjectContextResponse>, ApiError> {
    let project_id = uuid::Uuid::parse_str(&project_id).map_err(anyhow::Error::from)?;
    let project = state
        .store
        .get_project(project_id)?
        .ok_or_else(|| anyhow::anyhow!("project {project_id} not found"))?;
    let snapshot = ProjectContextService::default().discover(&project.workspace_path)?;
    let _ = state.store.update_project_agents_md(
        project.id,
        snapshot.root_agents_md.as_ref().map(|doc| doc.path.clone()),
        Some(snapshot.discovered_at),
    )?;
    Ok(Json(ProjectContextResponse { snapshot }))
}

async fn list_executors(
    State(state): State<AppState>,
) -> Result<Json<Vec<ExecutorProfile>>, ApiError> {
    Ok(Json(state.store.list_executor_profiles()?))
}

async fn list_executor_health() -> Result<Json<Vec<ExecutorHealth>>, ApiError> {
    Ok(Json(default_health_checks()))
}

async fn create_executor(
    State(state): State<AppState>,
    Json(input): Json<NewExecutorProfile>,
) -> Result<(StatusCode, Json<ExecutorProfile>), ApiError> {
    let profile = state.store.create_executor_profile(input)?;
    emit_event(
        &state,
        EventEnvelope::new(
            EventScope::Executor,
            "executor.created",
            format!("Created executor profile {}", profile.name),
            json!({"executor_profile_id": profile.id, "kind": profile.kind}),
        ),
    )?;
    Ok((StatusCode::CREATED, Json(profile)))
}

async fn list_roles(State(state): State<AppState>) -> Result<Json<Vec<AgentRole>>, ApiError> {
    Ok(Json(state.store.list_agent_roles()?))
}

async fn create_role(
    State(state): State<AppState>,
    Json(input): Json<NewAgentRole>,
) -> Result<(StatusCode, Json<AgentRole>), ApiError> {
    let role = state.store.create_agent_role(input)?;
    emit_event(
        &state,
        EventEnvelope::new(
            EventScope::System,
            "role.created",
            format!("Created role {}", role.name),
            json!({"role_id": role.id}),
        ),
    )?;
    Ok((StatusCode::CREATED, Json(role)))
}

async fn list_skills(State(state): State<AppState>) -> Result<Json<Vec<SkillDefinition>>, ApiError> {
    Ok(Json(state.store.list_skills()?))
}

async fn create_skill(
    State(state): State<AppState>,
    Json(input): Json<NewSkillDefinition>,
) -> Result<(StatusCode, Json<SkillDefinition>), ApiError> {
    let skill = state.store.create_skill(input)?;
    emit_event(
        &state,
        EventEnvelope::new(
            EventScope::System,
            "skill.created",
            format!("Created skill {}", skill.name),
            json!({"skill_id": skill.id}),
        ),
    )?;
    Ok((StatusCode::CREATED, Json(skill)))
}

async fn bind_skill_to_role(
    State(state): State<AppState>,
    Path(role_id): Path<String>,
    Json(input): Json<NewSkillBindingRequest>,
) -> Result<(StatusCode, Json<domain::SkillBinding>), ApiError> {
    let role_id = uuid::Uuid::parse_str(&role_id).map_err(anyhow::Error::from)?;
    let skill_id = uuid::Uuid::parse_str(&input.skill_id).map_err(anyhow::Error::from)?;
    let binding = state.store.bind_skill_to_role(role_id, skill_id)?;
    emit_event(
        &state,
        EventEnvelope::new(
            EventScope::System,
            "role.skill_bound",
            "Attached skill to role",
            json!({"role_id": role_id, "skill_id": skill_id}),
        ),
    )?;
    Ok((StatusCode::CREATED, Json(binding)))
}

async fn list_goals(State(state): State<AppState>) -> Result<Json<Vec<GoalSpec>>, ApiError> {
    Ok(Json(state.store.list_goals()?))
}

async fn create_goal(
    State(state): State<AppState>,
    Json(input): Json<NewGoalSpec>,
) -> Result<(StatusCode, Json<GoalSpec>), ApiError> {
    let goal = state.store.create_goal(input)?;
    emit_event(
        &state,
        EventEnvelope::new(
            EventScope::Workflow,
            "goal.created",
            format!("Created goal {}", goal.title),
            json!({"goal_id": goal.id, "kind": goal.kind.as_str()}),
        ),
    )?;
    Ok((StatusCode::CREATED, Json(goal)))
}

async fn compile_goal(
    State(state): State<AppState>,
    Path(goal_id): Path<String>,
    Json(input): Json<CompileGoalRequest>,
) -> Result<Json<CompiledGoal>, ApiError> {
    let goal_id = uuid::Uuid::parse_str(&goal_id).map_err(anyhow::Error::from)?;
    let compiled = SuperOwner::new(state.store.clone()).compile_goal(
        goal_id,
        input.agents_md_override.as_deref(),
    )?;
    emit_event(
        &state,
        EventEnvelope::new(
            EventScope::Workflow,
            "goal.compiled",
            format!("Compiled workflow {}", compiled.workflow.name),
            json!({"goal_id": compiled.goal.id, "workflow_id": compiled.workflow.id}),
        ),
    )?;
    Ok(Json(compiled))
}

async fn list_workflows(
    State(state): State<AppState>,
) -> Result<Json<Vec<WorkflowTemplate>>, ApiError> {
    Ok(Json(state.store.list_workflows()?))
}

async fn create_workflow(
    State(state): State<AppState>,
    Json(input): Json<NewWorkflowTemplate>,
) -> Result<(StatusCode, Json<WorkflowTemplate>), ApiError> {
    let workflow = state.store.create_workflow(input)?;
    emit_event(
        &state,
        EventEnvelope::new(
            EventScope::Workflow,
            "workflow.created",
            format!("Created workflow {}", workflow.name),
            json!({"workflow_id": workflow.id, "step_count": workflow.steps.len()}),
        ),
    )?;
    Ok((StatusCode::CREATED, Json(workflow)))
}

async fn list_runs(State(state): State<AppState>) -> Result<Json<Vec<Run>>, ApiError> {
    Ok(Json(state.store.list_runs()?))
}

async fn create_run(
    State(state): State<AppState>,
    Json(input): Json<NewRun>,
) -> Result<(StatusCode, Json<RunStateSnapshot>), ApiError> {
    let orchestrator = RunOrchestrator::new(state.store.clone(), state.events.clone());
    let snapshot = orchestrator.start_run(input)?;
    Ok((StatusCode::CREATED, Json(snapshot)))
}

async fn get_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunStateSnapshot>, ApiError> {
    let run_id = uuid::Uuid::parse_str(&run_id).map_err(anyhow::Error::from)?;
    let orchestrator = RunOrchestrator::new(state.store.clone(), state.events.clone());
    Ok(Json(orchestrator.snapshot(run_id)?))
}

async fn list_run_artifacts(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Vec<Artifact>>, ApiError> {
    let run_id = uuid::Uuid::parse_str(&run_id).map_err(anyhow::Error::from)?;
    Ok(Json(state.store.list_artifacts_for_run(run_id)?))
}

async fn complete_run_step(
    State(state): State<AppState>,
    Path((_run_id, run_step_id)): Path<(String, String)>,
) -> Result<Json<RunStateSnapshot>, ApiError> {
    let run_step_id = uuid::Uuid::parse_str(&run_step_id).map_err(anyhow::Error::from)?;
    let orchestrator = RunOrchestrator::new(state.store.clone(), state.events.clone());
    Ok(Json(orchestrator.complete_running_step(run_step_id)?))
}

async fn list_approvals(
    State(state): State<AppState>,
) -> Result<Json<Vec<ApprovalGate>>, ApiError> {
    Ok(Json(state.store.list_approval_gates()?))
}

async fn approve_gate(
    State(state): State<AppState>,
    Path(approval_id): Path<String>,
    Json(input): Json<ApprovalResolutionRequest>,
) -> Result<Json<RunStateSnapshot>, ApiError> {
    let approval_id = uuid::Uuid::parse_str(&approval_id).map_err(anyhow::Error::from)?;
    let orchestrator = RunOrchestrator::new(state.store.clone(), state.events.clone());
    Ok(Json(orchestrator.approve_gate(
        approval_id,
        input.resolved_by,
        input.notes,
    )?))
}

async fn reject_gate(
    State(state): State<AppState>,
    Path(approval_id): Path<String>,
    Json(input): Json<ApprovalResolutionRequest>,
) -> Result<Json<RunStateSnapshot>, ApiError> {
    let approval_id = uuid::Uuid::parse_str(&approval_id).map_err(anyhow::Error::from)?;
    let orchestrator = RunOrchestrator::new(state.store.clone(), state.events.clone());
    Ok(Json(orchestrator.reject_gate(
        approval_id,
        input.resolved_by,
        input.notes,
    )?))
}

async fn list_events(State(state): State<AppState>) -> Result<Json<Vec<EventEnvelope>>, ApiError> {
    Ok(Json(state.store.list_events()?))
}

async fn list_pairing_sessions(
    State(state): State<AppState>,
) -> Result<Json<Vec<PairingSession>>, ApiError> {
    Ok(Json(state.store.list_pairing_sessions()?))
}

async fn create_pairing_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PairingSessionCreateRequest>,
) -> Result<(StatusCode, Json<PairingSessionResponse>), ApiError> {
    let expires_at = input
        .expires_in_minutes
        .filter(|minutes| *minutes > 0)
        .map(|minutes| chrono::Utc::now() + chrono::Duration::minutes(minutes));
    let session = state
        .store
        .create_pairing_session(input.label, expires_at)?;
    let remote_url = build_remote_url(&headers, &session.token);

    emit_event(
        &state,
        EventEnvelope::new(
            EventScope::System,
            "pairing.created",
            "Created a remote browser pairing session",
            json!({
                "pairing_id": session.id,
                "remote_url": remote_url,
            }),
        ),
    )?;

    Ok((
        StatusCode::CREATED,
        Json(PairingSessionResponse {
            session,
            remote_url,
        }),
    ))
}

async fn revoke_pairing_session(
    State(state): State<AppState>,
    Path(pairing_id): Path<String>,
) -> Result<Json<PairingSession>, ApiError> {
    let pairing_id = uuid::Uuid::parse_str(&pairing_id).map_err(anyhow::Error::from)?;
    let session = state.store.revoke_pairing_session(pairing_id)?;

    emit_event(
        &state,
        EventEnvelope::new(
            EventScope::System,
            "pairing.revoked",
            "Revoked a remote browser pairing session",
            json!({
                "pairing_id": session.id,
            }),
        ),
    )?;

    Ok(Json(session))
}

async fn stream_events(
    State(state): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(state.events.subscribe()).filter_map(|item| async move {
        match item {
            Ok(event) => Event::default()
                .event(event.event_type.clone())
                .json_data(event)
                .ok()
                .map(Ok),
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}

async fn publish_test_event(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<EventEnvelope>), ApiError> {
    let event = EventEnvelope::new(
        EventScope::System,
        "system.test",
        "Published test event",
        json!({"source": "api"}),
    );
    emit_event(&state, event.clone())?;
    Ok((StatusCode::CREATED, Json(event)))
}

fn emit_event(state: &AppState, event: EventEnvelope) -> Result<(), ApiError> {
    state.store.record_event(&event)?;
    state.events.publish(event);
    Ok(())
}

async fn authorize_remote_api(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if is_local_host_request(&headers) {
        return Ok(next.run(request).await);
    }

    let token = headers
        .get("x-orch-pairing-token")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
        .or_else(|| token_from_query(&uri));

    match token {
        Some(token)
            if state
                .store
                .find_active_pairing_session_by_token(&token)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .is_some() =>
        {
            Ok(next.run(request).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

fn is_local_host_request(headers: &HeaderMap) -> bool {
    headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .map(|host| {
            host.starts_with("127.0.0.1")
                || host.starts_with("localhost")
                || host.starts_with("[::1]")
        })
        .unwrap_or(true)
}

fn token_from_query(uri: &Uri) -> Option<String> {
    uri.query().and_then(|query| {
        query.split('&').find_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next()?;
            let value = parts.next()?;
            if key == "token" {
                Some(value.to_string())
            } else {
                None
            }
        })
    })
}

fn build_remote_url(headers: &HeaderMap, token: &str) -> String {
    let host = headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("127.0.0.1:42420");
    let port = host.split(':').nth(1).unwrap_or("42420");
    let remote_host = detect_lan_ip().unwrap_or_else(|| "127.0.0.1".into());
    format!("http://{remote_host}:{port}/?token={token}")
}

fn detect_lan_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use domain::{ApprovalGate, Artifact, EventEnvelope, ExecutorKind, RunStatus};
    use observability::EventBus;
    use orchestrator::RunStateSnapshot;
    use persistence::OrchestratorStore;
    use tower::util::ServiceExt;

    use super::{app, AppState};

    #[tokio::test]
    async fn creates_and_lists_projects() {
        let state = AppState::in_memory().expect("state");
        let app = app(state);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "Mission Control",
                            "description": "Desktop control plane",
                            "workspace_path": "/workspace/mission-control",
                            "repository_url": "https://github.com/example/mission-control",
                            "default_executor_profile_id": null
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::CREATED);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/projects")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let projects: Vec<domain::Project> = serde_json::from_slice(&body).expect("projects");

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "Mission Control");
    }

    #[tokio::test]
    async fn creates_workflow_and_run_rows() {
        let state = AppState::in_memory().expect("state");
        let project = state
            .store
            .create_project(domain::NewProject {
                name: "Enterprise Orchestration".into(),
                description: None,
                workspace_path: "/workspace".into(),
                repository_url: None,
                default_executor_profile_id: None,
            })
            .expect("project");
        let executor = state
            .store
            .create_executor_profile(domain::NewExecutorProfile {
                name: "nca".into(),
                kind: ExecutorKind::NativeCliAi,
                binary_path: Some("nca".into()),
                config_json: serde_json::json!({}),
            })
            .expect("executor");

        let app = app(state.clone());

        let workflow_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/workflows")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": project.id,
                            "name": "Repo audit",
                            "description": "Plan and review",
                            "steps": [{
                                "name": "Plan",
                                "instruction": "Inspect the repo and create a plan",
                                "order_index": 0,
                                "executor_kind": "native_cli_ai",
                                "depends_on_step_id": null,
                                "timeout_seconds": 300,
                                "retry_limit": 1,
                                "requires_approval": true
                            }]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(workflow_response.status(), StatusCode::CREATED);

        let workflow_body = to_bytes(workflow_response.into_body(), usize::MAX)
            .await
            .expect("body");
        let workflow: domain::WorkflowTemplate =
            serde_json::from_slice(&workflow_body).expect("workflow");

        let run_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": project.id,
                            "workflow_template_id": workflow.id,
                            "executor_profile_id": executor.id,
                            "requested_by": "operator"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(run_response.status(), StatusCode::CREATED);

        let run_body = to_bytes(run_response.into_body(), usize::MAX)
            .await
            .expect("body");
        let snapshot: RunStateSnapshot = serde_json::from_slice(&run_body).expect("snapshot");
        assert_eq!(snapshot.run.status, RunStatus::WaitingForApproval);
        assert!(snapshot.pending_approval.is_some());

        let runs = state.store.list_runs().expect("runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id, snapshot.run.id);
    }

    #[tokio::test]
    async fn approval_action_endpoints_progress_runs() {
        let state = AppState::in_memory().expect("state");
        let project = state
            .store
            .create_project(domain::NewProject {
                name: "Approval project".into(),
                description: None,
                workspace_path: "/workspace".into(),
                repository_url: None,
                default_executor_profile_id: None,
            })
            .expect("project");
        let executor = state
            .store
            .create_executor_profile(domain::NewExecutorProfile {
                name: "nca".into(),
                kind: ExecutorKind::NativeCliAi,
                binary_path: Some("nca".into()),
                config_json: serde_json::json!({}),
            })
            .expect("executor");
        let workflow = state
            .store
            .create_workflow(domain::NewWorkflowTemplate {
                project_id: Some(project.id),
                name: "Approval flow".into(),
                description: None,
                steps: vec![domain::NewWorkflowStep {
                    name: "Gate".into(),
                    instruction: "Wait for approval".into(),
                    order_index: 0,
                    executor_kind: ExecutorKind::NativeCliAi,
                    role_id: None,
                    depends_on_step_id: None,
                    timeout_seconds: None,
                    retry_limit: 0,
                    requires_approval: true,
                    success_criteria: None,
                    artifact_contract: None,
                    input_schema: serde_json::json!({}),
                    output_schema: serde_json::json!({}),
                }],
            })
            .expect("workflow");

        let app = app(state.clone());
        let run_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": project.id,
                            "workflow_template_id": workflow.id,
                            "executor_profile_id": executor.id,
                            "requested_by": "operator"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        let run_body = to_bytes(run_response.into_body(), usize::MAX)
            .await
            .expect("body");
        let waiting: RunStateSnapshot = serde_json::from_slice(&run_body).expect("snapshot");
        let approval_id = waiting.pending_approval.expect("approval").id;
        let run_step_id = waiting.run_steps[0].id;

        let approve_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/approvals/{approval_id}/approve"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "resolved_by": "operator",
                            "notes": "Ship it"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(approve_response.status(), StatusCode::OK);
        let approve_body = to_bytes(approve_response.into_body(), usize::MAX)
            .await
            .expect("body");
        let approved: RunStateSnapshot =
            serde_json::from_slice(&approve_body).expect("approved snapshot");
        assert_eq!(approved.run.status, RunStatus::Running);

        let complete_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/runs/{}/steps/{run_step_id}/complete",
                        waiting.run.id
                    ))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(complete_response.status(), StatusCode::OK);
        let complete_body = to_bytes(complete_response.into_body(), usize::MAX)
            .await
            .expect("body");
        let completed: RunStateSnapshot =
            serde_json::from_slice(&complete_body).expect("completed snapshot");
        assert_eq!(completed.run.status, RunStatus::Completed);

        let artifact_response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/runs/{}/artifacts", waiting.run.id))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(artifact_response.status(), StatusCode::OK);
        let artifact_body = to_bytes(artifact_response.into_body(), usize::MAX)
            .await
            .expect("body");
        let artifacts: Vec<Artifact> = serde_json::from_slice(&artifact_body).expect("artifacts");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].kind, "summary");
    }

    #[tokio::test]
    async fn publishes_test_events_to_subscribers() {
        let state = AppState::in_memory().expect("state");
        let mut receiver = state.events.subscribe();
        let app = app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/events/test")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let event: EventEnvelope = serde_json::from_slice(&body).expect("event");
        assert_eq!(event.event_type, "system.test");

        let received = receiver.recv().await.expect("published event");
        assert_eq!(received.id, event.id);
    }

    #[tokio::test]
    async fn approvals_endpoint_returns_json() {
        let state = AppState::in_memory().expect("state");
        let app = app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/approvals")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let approvals: Vec<ApprovalGate> = serde_json::from_slice(&body).expect("approvals");
        assert!(approvals.is_empty());
    }

    #[tokio::test]
    async fn remote_api_requests_require_pairing_token() {
        let state = AppState::in_memory().expect("state");
        let app = app(state.clone());

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/projects")
                    .header("host", "192.168.1.10:42420")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let pairing_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/pairing-sessions")
                    .header("host", "127.0.0.1:42420")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "label": "Phone",
                            "expires_in_minutes": 30
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(pairing_response.status(), StatusCode::CREATED);
        let pairing_body = to_bytes(pairing_response.into_body(), usize::MAX)
            .await
            .expect("body");
        let pairing: super::PairingSessionResponse =
            serde_json::from_slice(&pairing_body).expect("pairing");

        let authorized = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/projects")
                    .header("host", "192.168.1.10:42420")
                    .header("x-orch-pairing-token", pairing.session.token)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(authorized.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn serves_frontend_index_when_dist_is_configured() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let frontend_dist = manifest_dir
            .join("../../../apps/control-ui/dist")
            .canonicalize()
            .ok();

        if frontend_dist.is_none() {
            eprintln!("Skipping test: apps/control-ui/dist not found");
            return;
        }

        let state = AppState {
            store: OrchestratorStore::open_in_memory().expect("store"),
            events: EventBus::default(),
            frontend_dist,
        };
        let app = app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let html = String::from_utf8(body.to_vec()).expect("html");
        assert!(html.contains("<div id=\"root\"></div>"));
    }
}
