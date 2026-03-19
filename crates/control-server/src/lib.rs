use std::{convert::Infallible, time::Duration};

use anyhow::Result;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use domain::{
    ApprovalGate, EventEnvelope, EventScope, ExecutorProfile, NewExecutorProfile, NewProject,
    NewRun, NewWorkflowTemplate, Project, Run, WorkflowTemplate,
};
use futures_util::StreamExt;
use observability::EventBus;
use orchestrator::{RunOrchestrator, RunStateSnapshot};
use persistence::OrchestratorStore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

#[derive(Clone)]
pub struct AppState {
    pub store: OrchestratorStore,
    pub events: EventBus,
}

impl AppState {
    pub fn in_memory() -> Result<Self> {
        Ok(Self {
            store: OrchestratorStore::open_in_memory()?,
            events: EventBus::default(),
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
    Router::new()
        .route("/health", get(health))
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/executors", get(list_executors).post(create_executor))
        .route("/api/workflows", get(list_workflows).post(create_workflow))
        .route("/api/runs", get(list_runs).post(create_run))
        .route("/api/runs/{run_id}", get(get_run))
        .route(
            "/api/runs/{run_id}/steps/{run_step_id}/complete",
            post(complete_run_step),
        )
        .route("/api/approvals", get(list_approvals))
        .route("/api/approvals/{approval_id}/approve", post(approve_gate))
        .route("/api/approvals/{approval_id}/reject", post(reject_gate))
        .route("/api/events", get(list_events))
        .route("/api/events/stream", get(stream_events))
        .route("/api/events/test", post(publish_test_event))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn list_projects(State(state): State<AppState>) -> Result<Json<Vec<Project>>, ApiError> {
    Ok(Json(state.store.list_projects()?))
}

async fn create_project(
    State(state): State<AppState>,
    Json(input): Json<NewProject>,
) -> Result<(StatusCode, Json<Project>), ApiError> {
    let project = state.store.create_project(input)?;
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

async fn list_executors(
    State(state): State<AppState>,
) -> Result<Json<Vec<ExecutorProfile>>, ApiError> {
    Ok(Json(state.store.list_executor_profiles()?))
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

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use domain::{ApprovalGate, EventEnvelope, ExecutorKind, RunStatus};
    use orchestrator::RunStateSnapshot;
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
                    depends_on_step_id: None,
                    timeout_seconds: None,
                    retry_limit: 0,
                    requires_approval: true,
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
}
