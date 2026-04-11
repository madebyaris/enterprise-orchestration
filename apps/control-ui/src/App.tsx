import { useCallback, useEffect, useMemo, useState } from 'react'
import QRCode from 'qrcode'
import './App.css'

type ExecutorKind =
  | 'native_cli_ai'
  | 'claude_code'
  | 'codex'
  | 'opencode'
  | 'shell'

type RunStatus =
  | 'queued'
  | 'running'
  | 'waiting_for_approval'
  | 'completed'
  | 'failed'
  | 'cancelled'

type RunStepStatus = RunStatus | 'pending'

type Project = {
  id: string
  name: string
  description?: string | null
  workspace_path: string
  repository_url?: string | null
  agents_md_path?: string | null
}

type ExecutorProfile = {
  id: string
  name: string
  kind: ExecutorKind
  binary_path?: string | null
}

type ExecutorHealth = {
  kind: ExecutorKind
  available: boolean
  binary_path?: string | null
  version_hint?: string | null
}

type AgentRole = {
  id: string
  name: string
  description?: string | null
  system_prompt: string
  default_executor_kind?: ExecutorKind | null
}

type SkillDefinition = {
  id: string
  name: string
  description?: string | null
  instructions: string
  source: 'inline' | 'agents_md' | 'file' | 'remote'
  source_uri?: string | null
}

type GoalSpec = {
  id: string
  project_id: string
  kind: 'create_app' | 'create_workflow'
  title: string
  prompt: string
  status: 'draft' | 'compiled' | 'running' | 'completed' | 'failed'
  compiled_workflow_template_id?: string | null
}

type WorkflowStep = {
  id: string
  workflow_template_id: string
  name: string
  instruction: string
  order_index: number
  executor_kind: ExecutorKind
  role_id?: string | null
  requires_approval: boolean
  success_criteria?: string | null
  artifact_contract?: string | null
}

type WorkflowTemplate = {
  id: string
  project_id?: string | null
  name: string
  description?: string | null
  steps: WorkflowStep[]
}

type Run = {
  id: string
  project_id: string
  workflow_template_id: string
  executor_profile_id?: string | null
  status: RunStatus
  requested_by?: string | null
}

type RunStep = {
  id: string
  run_id: string
  workflow_step_id: string
  status: RunStepStatus
  external_session_id?: string | null
}

type ApprovalGate = {
  id: string
  run_id: string
  run_step_id?: string | null
  status: 'pending' | 'approved' | 'rejected'
  notes?: string | null
  requested_by?: string | null
  resolved_by?: string | null
}

type PairingSession = {
  id: string
  token: string
  label?: string | null
  is_revoked: boolean
  created_at: string
  expires_at?: string | null
}

type PairingSessionResponse = {
  session: PairingSession
  remote_url: string
}

type Artifact = {
  id: string
  run_id: string
  run_step_id?: string | null
  name: string
  kind: string
  content_type?: string | null
  metadata_json: Record<string, unknown>
}

type EventEnvelope = {
  id: string
  event_type: string
  summary: string
  created_at: string
  payload: Record<string, unknown>
}

type RunSnapshot = {
  run: Run
  run_steps: RunStep[]
  workflow_steps: WorkflowStep[]
  pending_approval?: ApprovalGate | null
}

type DashboardData = {
  projects: Project[]
  roles: AgentRole[]
  skills: SkillDefinition[]
  goals: GoalSpec[]
  executors: ExecutorProfile[]
  executorHealth: ExecutorHealth[]
  workflows: WorkflowTemplate[]
  runs: Run[]
  approvals: ApprovalGate[]
  pairings: PairingSession[]
  events: EventEnvelope[]
}

type AgentsSection = {
  heading: string
  content: string
}

type AgentsDocument = {
  path: string
  instructions: string
  sections: AgentsSection[]
}

type ProjectContextSnapshot = {
  workspace_path: string
  root_agents_md?: AgentsDocument | null
  nested_agents_md: AgentsDocument[]
  discovered_at: string
}

type ProjectContextResponse = {
  snapshot: ProjectContextSnapshot
}

type CompiledGoal = {
  goal: GoalSpec
  project: Project
  workflow: WorkflowTemplate
  agents_md?: string | null
}

type ViewKey =
  | 'overview'
  | 'projects'
  | 'roles'
  | 'skills'
  | 'goals'
  | 'executors'
  | 'workflows'
  | 'runs'

const DEFAULT_API_BASE = 'http://127.0.0.1:42420'

function resolveApiBase() {
  const injectedBase = import.meta.env.VITE_CONTROL_SERVER_URL as string | undefined
  if (injectedBase) {
    return injectedBase.replace(/\/$/, '')
  }

  if (
    typeof window !== 'undefined' &&
    (window.location.protocol === 'http:' || window.location.protocol === 'https:')
  ) {
    if (window.location.port !== '1420' && window.location.port !== '5173') {
      return window.location.origin
    }
  }

  return DEFAULT_API_BASE
}

const API_BASE = resolveApiBase()

async function requestJson<T>(path: string, init?: RequestInit) {
  const pairingToken =
    typeof window !== 'undefined'
      ? new URLSearchParams(window.location.search).get('token')
      : null

  const response = await fetch(`${API_BASE}${path}`, {
    headers: {
      'Content-Type': 'application/json',
      ...(pairingToken ? { 'x-orch-pairing-token': pairingToken } : {}),
      ...(init?.headers ?? {}),
    },
    ...init,
  })

  if (!response.ok) {
    const text = await response.text()
    throw new Error(text || `Request failed with ${response.status}`)
  }

  return (await response.json()) as T
}

function prettyStatus(value: string) {
  return value.replaceAll('_', ' ')
}

function App() {
  const [view, setView] = useState<ViewKey>('overview')
  const [dashboard, setDashboard] = useState<DashboardData>({
    projects: [],
    roles: [],
    skills: [],
    goals: [],
    executors: [],
    executorHealth: [],
    workflows: [],
    runs: [],
    approvals: [],
    pairings: [],
    events: [],
  })
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null)
  const [selectedRun, setSelectedRun] = useState<RunSnapshot | null>(null)
  const [selectedArtifacts, setSelectedArtifacts] = useState<Artifact[]>([])
  const [loading, setLoading] = useState(true)
  const [submitting, setSubmitting] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [successMessage, setSuccessMessage] = useState<string | null>(null)
  const [lastRefreshedAt, setLastRefreshedAt] = useState<Date | null>(null)
  const [activePairingUrl, setActivePairingUrl] = useState<string | null>(null)
  const [pairingQrDataUrl, setPairingQrDataUrl] = useState<string | null>(null)
  const [selectedProjectContext, setSelectedProjectContext] = useState<ProjectContextSnapshot | null>(null)
  const [selectedContextProjectId, setSelectedContextProjectId] = useState<string | null>(null)
  const [compiledGoal, setCompiledGoal] = useState<CompiledGoal | null>(null)

  const [projectForm, setProjectForm] = useState({
    name: '',
    description: '',
    workspacePath: '/workspace',
    repositoryUrl: '',
  })
  const [executorForm, setExecutorForm] = useState({
    name: 'native-cli-ai',
    kind: 'native_cli_ai' as ExecutorKind,
    binaryPath: 'nca',
  })
  const [roleForm, setRoleForm] = useState({
    name: 'CEO',
    description: 'Sets direction and product goals.',
    systemPrompt: 'Define direction, constraints, and desired outcomes.',
    defaultExecutorKind: 'claude_code' as ExecutorKind,
  })
  const [skillForm, setSkillForm] = useState({
    name: 'Repository guidance',
    description: 'Reusable project guidance.',
    instructions: 'Read and follow the repository guidance before making changes.',
    source: 'inline' as SkillDefinition['source'],
    sourceUri: '',
  })
  const [roleSkillForm, setRoleSkillForm] = useState({
    roleId: '',
    skillId: '',
  })
  const [goalForm, setGoalForm] = useState({
    projectId: '',
    kind: 'create_app' as GoalSpec['kind'],
    title: 'Create app',
    prompt: 'Create an app that matches the project goal.',
  })
  const [workflowForm, setWorkflowForm] = useState({
    projectId: '',
    name: 'Repository audit',
    description: 'Inspect the repository and create an operator-ready plan.',
    stepName: 'Inspect repository',
    stepInstruction: 'Inspect the repository and create an operator-ready plan.',
    executorKind: 'native_cli_ai' as ExecutorKind,
    roleId: '',
    requiresApproval: true,
  })
  const [runForm, setRunForm] = useState({
    projectId: '',
    workflowTemplateId: '',
    executorProfileId: '',
    requestedBy: 'operator',
  })
  const [pairingForm, setPairingForm] = useState({
    label: 'Phone control',
    expiresInMinutes: 60,
  })

  const refreshDashboard = useCallback(async () => {
    const [projects, roles, skills, goals, executors, executorHealth, workflows, runs, approvals, pairings, events] =
      await Promise.all([
      requestJson<Project[]>('/api/projects'),
      requestJson<AgentRole[]>('/api/roles'),
      requestJson<SkillDefinition[]>('/api/skills'),
      requestJson<GoalSpec[]>('/api/goals'),
      requestJson<ExecutorProfile[]>('/api/executors'),
      requestJson<ExecutorHealth[]>('/api/executors/health'),
      requestJson<WorkflowTemplate[]>('/api/workflows'),
      requestJson<Run[]>('/api/runs'),
      requestJson<ApprovalGate[]>('/api/approvals'),
      requestJson<PairingSession[]>('/api/pairing-sessions'),
      requestJson<EventEnvelope[]>('/api/events'),
    ])

    setDashboard({
      projects,
      roles,
      skills,
      goals,
      executors,
      executorHealth,
      workflows,
      runs,
      approvals,
      pairings,
      events,
    })
    setLastRefreshedAt(new Date())

    if (!selectedRunId && runs[0]) {
      setSelectedRunId(runs[0].id)
    }
  }, [selectedRunId])

  const refreshSelectedRun = useCallback(async () => {
    if (!selectedRunId) {
      setSelectedRun(null)
      setSelectedArtifacts([])
      return
    }

    try {
      const [snapshot, artifacts] = await Promise.all([
        requestJson<RunSnapshot>(`/api/runs/${selectedRunId}`),
        requestJson<Artifact[]>(`/api/runs/${selectedRunId}/artifacts`),
      ])
      setSelectedRun(snapshot)
      setSelectedArtifacts(artifacts)
    } catch (runError) {
      console.warn(runError)
    }
  }, [selectedRunId])

  useEffect(() => {
    let mounted = true

    const load = async () => {
      try {
        setError(null)
        await refreshDashboard()
        if (mounted) {
          setLoading(false)
        }
      } catch (loadError) {
        if (mounted) {
          setError(loadError instanceof Error ? loadError.message : String(loadError))
          setLoading(false)
        }
      }
    }

    load()
    const interval = window.setInterval(() => {
      void refreshDashboard()
      void refreshSelectedRun()
    }, 4000)

    return () => {
      mounted = false
      window.clearInterval(interval)
    }
  }, [refreshDashboard, refreshSelectedRun])

  useEffect(() => {
    void refreshSelectedRun()
  }, [refreshSelectedRun])

  useEffect(() => {
    if (!activePairingUrl) {
      setPairingQrDataUrl(null)
      return
    }

    let cancelled = false
    QRCode.toDataURL(activePairingUrl, {
      color: {
        dark: '#f8fafc',
        light: '#0000',
      },
      margin: 1,
      width: 220,
    })
      .then((value: string) => {
        if (!cancelled) {
          setPairingQrDataUrl(value)
        }
      })
      .catch((error: unknown) => {
        console.warn(error)
        setPairingQrDataUrl(null)
      })

    return () => {
      cancelled = true
    }
  }, [activePairingUrl])

  useEffect(() => {
    if (!goalForm.projectId && dashboard.projects[0]) {
      setGoalForm((current) => ({ ...current, projectId: dashboard.projects[0].id }))
    }

    if (!roleSkillForm.roleId && dashboard.roles[0]) {
      setRoleSkillForm((current) => ({ ...current, roleId: dashboard.roles[0].id }))
    }

    if (!roleSkillForm.skillId && dashboard.skills[0]) {
      setRoleSkillForm((current) => ({ ...current, skillId: dashboard.skills[0].id }))
    }

    if (!workflowForm.projectId && dashboard.projects[0]) {
      setWorkflowForm((current) => ({ ...current, projectId: dashboard.projects[0].id }))
    }

    if (!runForm.projectId && dashboard.projects[0]) {
      setRunForm((current) => ({ ...current, projectId: dashboard.projects[0].id }))
    }

    if (!runForm.workflowTemplateId && dashboard.workflows[0]) {
      setRunForm((current) => ({
        ...current,
        workflowTemplateId: dashboard.workflows[0].id,
      }))
    }

    if (!runForm.executorProfileId && dashboard.executors[0]) {
      setRunForm((current) => ({
        ...current,
        executorProfileId: dashboard.executors[0].id,
      }))
    }
    if (!workflowForm.roleId && dashboard.roles[0]) {
      setWorkflowForm((current) => ({ ...current, roleId: dashboard.roles[0].id }))
    }
  }, [
    dashboard,
    goalForm.projectId,
    roleSkillForm.roleId,
    roleSkillForm.skillId,
    runForm.executorProfileId,
    runForm.projectId,
    runForm.workflowTemplateId,
    workflowForm.projectId,
    workflowForm.roleId,
  ])

  const projectIndex = useMemo(
    () => Object.fromEntries(dashboard.projects.map((project) => [project.id, project])),
    [dashboard.projects],
  )
  const workflowIndex = useMemo(
    () => Object.fromEntries(dashboard.workflows.map((workflow) => [workflow.id, workflow])),
    [dashboard.workflows],
  )
  const executorIndex = useMemo(
    () => Object.fromEntries(dashboard.executors.map((executor) => [executor.id, executor])),
    [dashboard.executors],
  )
  const roleIndex = useMemo(
    () => Object.fromEntries(dashboard.roles.map((role) => [role.id, role])),
    [dashboard.roles],
  )

  const handleAction = useCallback(
    async (actionKey: string, fn: () => Promise<void>) => {
      try {
        setSubmitting(actionKey)
        setError(null)
        setSuccessMessage(null)
        await fn()
        await refreshDashboard()
        await refreshSelectedRun()
      } catch (actionError) {
        setError(actionError instanceof Error ? actionError.message : String(actionError))
      } finally {
        setSubmitting(null)
      }
    },
    [refreshDashboard, refreshSelectedRun],
  )

  const createProject = async () =>
    handleAction('create-project', async () => {
      const project = await requestJson<Project>('/api/projects', {
        method: 'POST',
        body: JSON.stringify({
          name: projectForm.name,
          description: projectForm.description || null,
          workspace_path: projectForm.workspacePath,
          repository_url: projectForm.repositoryUrl || null,
          default_executor_profile_id: null,
        }),
      })
      setSuccessMessage(`Created project ${project.name}`)
      setProjectForm((current) => ({ ...current, name: '', description: '', repositoryUrl: '' }))
    })

  const createExecutor = async () =>
    handleAction('create-executor', async () => {
      const executor = await requestJson<ExecutorProfile>('/api/executors', {
        method: 'POST',
        body: JSON.stringify({
          name: executorForm.name,
          kind: executorForm.kind,
          binary_path: executorForm.binaryPath || null,
          config_json: {},
        }),
      })
      setSuccessMessage(`Created executor profile ${executor.name}`)
    })

  const createRole = async () =>
    handleAction('create-role', async () => {
      const role = await requestJson<AgentRole>('/api/roles', {
        method: 'POST',
        body: JSON.stringify({
          name: roleForm.name,
          description: roleForm.description || null,
          system_prompt: roleForm.systemPrompt,
          default_executor_kind: roleForm.defaultExecutorKind,
        }),
      })
      setRoleSkillForm((current) => ({ ...current, roleId: role.id }))
      setSuccessMessage(`Created role ${role.name}`)
    })

  const createSkill = async () =>
    handleAction('create-skill', async () => {
      const skill = await requestJson<SkillDefinition>('/api/skills', {
        method: 'POST',
        body: JSON.stringify({
          name: skillForm.name,
          description: skillForm.description || null,
          instructions: skillForm.instructions,
          source: skillForm.source,
          source_uri: skillForm.sourceUri || null,
        }),
      })
      setRoleSkillForm((current) => ({ ...current, skillId: skill.id }))
      setSuccessMessage(`Created skill ${skill.name}`)
    })

  const bindSkillToRole = async () =>
    handleAction('bind-skill', async () => {
      await requestJson(`/api/roles/${roleSkillForm.roleId}/skills`, {
        method: 'POST',
        body: JSON.stringify({
          skill_id: roleSkillForm.skillId,
        }),
      })
      setSuccessMessage('Attached skill to role')
    })

  const createGoal = async () =>
    handleAction('create-goal', async () => {
      const goal = await requestJson<GoalSpec>('/api/goals', {
        method: 'POST',
        body: JSON.stringify({
          project_id: goalForm.projectId,
          kind: goalForm.kind,
          title: goalForm.title,
          prompt: goalForm.prompt,
        }),
      })
      setSuccessMessage(`Created goal ${goal.title}`)
    })

  const compileGoal = async (goalId: string) =>
    handleAction(`compile-goal-${goalId}`, async () => {
      const response = await requestJson<CompiledGoal>(`/api/goals/${goalId}/compile`, {
        method: 'POST',
        body: JSON.stringify({
          agents_md_override:
            selectedProjectContext?.root_agents_md?.instructions || null,
        }),
      })
      setCompiledGoal(response)
      setSuccessMessage(`Compiled workflow ${response.workflow.name}`)
    })

  const inspectProjectContext = async (projectId: string) =>
    handleAction(`project-context-${projectId}`, async () => {
      const response = await requestJson<ProjectContextResponse>(`/api/projects/${projectId}/context`)
      setSelectedContextProjectId(projectId)
      setSelectedProjectContext(response.snapshot)
      setSuccessMessage('Loaded project guidance from AGENTS.md')
    })

  const createWorkflow = async () =>
    handleAction('create-workflow', async () => {
      const workflow = await requestJson<WorkflowTemplate>('/api/workflows', {
        method: 'POST',
        body: JSON.stringify({
          project_id: workflowForm.projectId || null,
          name: workflowForm.name,
          description: workflowForm.description || null,
          steps: [
            {
              name: workflowForm.stepName,
              instruction: workflowForm.stepInstruction,
              order_index: 0,
              executor_kind: workflowForm.executorKind,
              role_id: workflowForm.roleId || null,
              depends_on_step_id: null,
              timeout_seconds: 300,
              retry_limit: 1,
              requires_approval: workflowForm.requiresApproval,
              success_criteria: null,
              artifact_contract: null,
              input_schema: {},
              output_schema: {},
            },
          ],
        }),
      })
      setSuccessMessage(`Created workflow ${workflow.name}`)
      setRunForm((current) => ({ ...current, workflowTemplateId: workflow.id }))
    })

  const createRun = async () =>
    handleAction('create-run', async () => {
      const snapshot = await requestJson<RunSnapshot>('/api/runs', {
        method: 'POST',
        body: JSON.stringify({
          project_id: runForm.projectId,
          workflow_template_id: runForm.workflowTemplateId,
          executor_profile_id: runForm.executorProfileId || null,
          requested_by: runForm.requestedBy || null,
        }),
      })
      setSelectedRunId(snapshot.run.id)
      setSelectedRun(snapshot)
      setSelectedArtifacts([])
      setView('runs')
      setSuccessMessage(`Started run ${snapshot.run.id.slice(0, 8)}`)
    })

  const createPairingSession = async () =>
    handleAction('create-pairing', async () => {
      const response = await requestJson<PairingSessionResponse>('/api/pairing-sessions', {
        method: 'POST',
        body: JSON.stringify({
          label: pairingForm.label || null,
          expires_in_minutes: pairingForm.expiresInMinutes,
        }),
      })
      setActivePairingUrl(response.remote_url)
      setSuccessMessage('Created a phone pairing link')
    })

  const revokePairingSession = async (pairingId: string) =>
    handleAction(`revoke-pairing-${pairingId}`, async () => {
      await requestJson<PairingSession>(`/api/pairing-sessions/${pairingId}/revoke`, {
        method: 'POST',
      })

      if (dashboard.pairings.find((session) => session.id === pairingId)?.is_revoked === false) {
        setActivePairingUrl(null)
        setPairingQrDataUrl(null)
      }
      setSuccessMessage('Revoked remote browser access')
    })

  const completeStep = async (runStepId: string) =>
    handleAction(`complete-step-${runStepId}`, async () => {
      if (!selectedRun) return
      const snapshot = await requestJson<RunSnapshot>(
        `/api/runs/${selectedRun.run.id}/steps/${runStepId}/complete`,
        {
          method: 'POST',
        },
      )
      setSelectedRun(snapshot)
      setSuccessMessage('Completed the active workflow step')
    })

  const resolveApproval = async (decision: 'approve' | 'reject') =>
    handleAction(`${decision}-approval`, async () => {
      if (!selectedRun?.pending_approval) return
      const snapshot = await requestJson<RunSnapshot>(
        `/api/approvals/${selectedRun.pending_approval.id}/${decision}`,
        {
          method: 'POST',
          body: JSON.stringify({
            resolved_by: runForm.requestedBy || 'operator',
            notes: decision === 'approve' ? 'Approved from dashboard' : 'Rejected from dashboard',
          }),
        },
      )
      setSelectedRun(snapshot)
      setSuccessMessage(
        decision === 'approve' ? 'Approved the pending step' : 'Rejected the pending step',
      )
    })

  const activeRunCount = dashboard.runs.filter((run) =>
    ['queued', 'running', 'waiting_for_approval'].includes(run.status),
  ).length

  const completedRunCount = dashboard.runs.filter((run) => run.status === 'completed').length

  const activeStep = selectedRun?.run_steps.find((step) =>
    ['running', 'waiting_for_approval'].includes(step.status),
  )
  const activeWorkflowStep = selectedRun?.workflow_steps.find(
    (step) => step.id === activeStep?.workflow_step_id,
  )

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark">EO</span>
          <div>
            <p className="eyebrow">Desktop control plane</p>
            <h1>Enterprise Orchestration</h1>
          </div>
        </div>

        <nav className="nav-list" aria-label="Primary navigation">
          {(
            ['overview', 'projects', 'roles', 'skills', 'goals', 'executors', 'workflows', 'runs'] as ViewKey[]
          ).map((item) => (
            <button
              key={item}
              className={`nav-item ${view === item ? 'active' : ''}`}
              onClick={() => setView(item)}
            >
              {item}
            </button>
          ))}
        </nav>

        <div className="sidebar-card">
          <p className="eyebrow">Connection</p>
          <strong>{API_BASE}</strong>
          <span className="muted">
            {lastRefreshedAt ? `Refreshed ${lastRefreshedAt.toLocaleTimeString()}` : 'Connecting…'}
          </span>
        </div>
      </aside>

      <main className="main-content">
        <header className="topbar">
          <div>
            <p className="eyebrow">Mission control dashboard</p>
            <h2>Operate local-first AI workflows from desktop and phone</h2>
          </div>
          <div className="topbar-actions">
            <button className="ghost-button" onClick={() => void refreshDashboard()}>
              Refresh
            </button>
            <span className="status-chip healthy">{loading ? 'Loading' : 'Connected'}</span>
          </div>
        </header>

        {error ? <div className="alert error">{error}</div> : null}
        {successMessage ? <div className="alert success">{successMessage}</div> : null}

        <section className="stats-grid">
          <article className="stat-card">
            <span className="stat-label">Projects</span>
            <strong>{dashboard.projects.length}</strong>
            <span className="muted">Connected workspaces and repositories</span>
          </article>
          <article className="stat-card">
            <span className="stat-label">Executors</span>
            <strong>{dashboard.executors.length}</strong>
            <span className="muted">CLI adapters available to workflows</span>
          </article>
          <article className="stat-card">
            <span className="stat-label">Active runs</span>
            <strong>{activeRunCount}</strong>
            <span className="muted">Queued, running, or waiting on approval</span>
          </article>
          <article className="stat-card">
            <span className="stat-label">Completed runs</span>
            <strong>{completedRunCount}</strong>
            <span className="muted">Historical execution successes</span>
          </article>
        </section>

        <section className="content-grid">
          <div className="primary-column">
            {(view === 'overview' || view === 'projects') && (
              <section className="panel">
                <div className="panel-header">
                  <div>
                    <p className="eyebrow">Projects</p>
                    <h3>Manage local repositories</h3>
                  </div>
                  <span className="status-chip neutral">{dashboard.projects.length} total</span>
                </div>

                <div className="form-grid">
                  <input
                    value={projectForm.name}
                    onChange={(event) =>
                      setProjectForm((current) => ({ ...current, name: event.target.value }))
                    }
                    placeholder="Project name"
                  />
                  <input
                    value={projectForm.workspacePath}
                    onChange={(event) =>
                      setProjectForm((current) => ({
                        ...current,
                        workspacePath: event.target.value,
                      }))
                    }
                    placeholder="Workspace path"
                  />
                  <input
                    value={projectForm.repositoryUrl}
                    onChange={(event) =>
                      setProjectForm((current) => ({
                        ...current,
                        repositoryUrl: event.target.value,
                      }))
                    }
                    placeholder="Repository URL"
                  />
                  <input
                    value={projectForm.description}
                    onChange={(event) =>
                      setProjectForm((current) => ({
                        ...current,
                        description: event.target.value,
                      }))
                    }
                    placeholder="Description"
                  />
                </div>

                <button
                  className="primary-button"
                  disabled={!projectForm.name || !projectForm.workspacePath || submitting !== null}
                  onClick={() => void createProject()}
                >
                  {submitting === 'create-project' ? 'Creating…' : 'Create project'}
                </button>

                <div className="table-list">
                  {dashboard.projects.map((project) => (
                    <article key={project.id} className="list-row stacked">
                      <div>
                        <strong>{project.name}</strong>
                        <p>{project.workspace_path}</p>
                      </div>
                      <div className="row-tags">
                        <span className="muted">{project.repository_url || 'local-only'}</span>
                        <span className="status-chip neutral">
                          {project.agents_md_path ? 'AGENTS.md detected' : 'no AGENTS.md'}
                        </span>
                      </div>
                      <button
                        className="ghost-button"
                        disabled={submitting !== null}
                        onClick={() => void inspectProjectContext(project.id)}
                      >
                        {submitting === `project-context-${project.id}`
                          ? 'Inspecting…'
                          : 'Inspect AGENTS.md'}
                      </button>
                    </article>
                  ))}
                  {!dashboard.projects.length && (
                    <div className="empty-state">No projects yet. Create one to get started.</div>
                  )}
                </div>

                {selectedProjectContext && (
                  <div className="mini-card">
                    <strong>
                      Project guidance
                      {selectedContextProjectId
                        ? ` for ${projectIndex[selectedContextProjectId]?.name || 'project'}`
                        : ''}
                    </strong>
                    <p>
                      Root file:{' '}
                      {selectedProjectContext.root_agents_md?.path || 'No root AGENTS.md found'}
                    </p>
                    <p>{selectedProjectContext.root_agents_md?.instructions || 'No AGENTS guidance loaded yet.'}</p>
                  </div>
                )}
              </section>
            )}

            {(view === 'overview' || view === 'executors') && (
              <section className="panel">
                <div className="panel-header">
                  <div>
                    <p className="eyebrow">Executors</p>
                    <h3>Configure CLI runners</h3>
                  </div>
                  <span className="status-chip neutral">{dashboard.executors.length} profiles</span>
                </div>

                <div className="form-grid">
                  <input
                    value={executorForm.name}
                    onChange={(event) =>
                      setExecutorForm((current) => ({ ...current, name: event.target.value }))
                    }
                    placeholder="Executor profile name"
                  />
                  <select
                    value={executorForm.kind}
                    onChange={(event) =>
                      setExecutorForm((current) => ({
                        ...current,
                        kind: event.target.value as ExecutorKind,
                      }))
                    }
                  >
                    <option value="native_cli_ai">native-cli-ai</option>
                    <option value="claude_code">Claude Code</option>
                    <option value="codex">Codex CLI</option>
                    <option value="opencode">OpenCode</option>
                    <option value="shell">Shell</option>
                  </select>
                  <input
                    value={executorForm.binaryPath}
                    onChange={(event) =>
                      setExecutorForm((current) => ({
                        ...current,
                        binaryPath: event.target.value,
                      }))
                    }
                    placeholder="Binary path"
                  />
                </div>

                <button
                  className="primary-button"
                  disabled={!executorForm.name || submitting !== null}
                  onClick={() => void createExecutor()}
                >
                  {submitting === 'create-executor' ? 'Creating…' : 'Create executor'}
                </button>

                <div className="card-grid">
                  {dashboard.executors.map((executor) => (
                    <article key={executor.id} className="mini-card">
                      <strong>{executor.name}</strong>
                      <span className="status-chip neutral">{prettyStatus(executor.kind)}</span>
                      <p>{executor.binary_path || 'Using PATH lookup'}</p>
                      <p>
                        Health:{' '}
                        {dashboard.executorHealth.find((health) => health.kind === executor.kind)?.available
                          ? 'available'
                          : 'unavailable'}
                      </p>
                    </article>
                  ))}
                  {!dashboard.executors.length && (
                    <div className="empty-state">No executor profiles configured yet.</div>
                  )}
                </div>
              </section>
            )}

            {(view === 'overview' || view === 'roles') && (
              <section className="panel">
                <div className="panel-header">
                  <div>
                    <p className="eyebrow">Roles</p>
                    <h3>Define CEO, PM, Engineer, Reviewer templates</h3>
                  </div>
                  <span className="status-chip neutral">{dashboard.roles.length} roles</span>
                </div>

                <div className="form-grid">
                  <input
                    value={roleForm.name}
                    onChange={(event) =>
                      setRoleForm((current) => ({ ...current, name: event.target.value }))
                    }
                    placeholder="Role name"
                  />
                  <select
                    value={roleForm.defaultExecutorKind}
                    onChange={(event) =>
                      setRoleForm((current) => ({
                        ...current,
                        defaultExecutorKind: event.target.value as ExecutorKind,
                      }))
                    }
                  >
                    <option value="native_cli_ai">native-cli-ai</option>
                    <option value="claude_code">Claude Code</option>
                    <option value="codex">Codex CLI</option>
                    <option value="opencode">OpenCode</option>
                    <option value="shell">Shell</option>
                  </select>
                  <input
                    value={roleForm.description}
                    onChange={(event) =>
                      setRoleForm((current) => ({ ...current, description: event.target.value }))
                    }
                    placeholder="Role description"
                  />
                </div>
                <textarea
                  value={roleForm.systemPrompt}
                  onChange={(event) =>
                    setRoleForm((current) => ({ ...current, systemPrompt: event.target.value }))
                  }
                  placeholder="Role system prompt"
                />
                <button
                  className="primary-button"
                  disabled={!roleForm.name || !roleForm.systemPrompt || submitting !== null}
                  onClick={() => void createRole()}
                >
                  {submitting === 'create-role' ? 'Creating…' : 'Create role'}
                </button>

                <div className="card-grid">
                  {dashboard.roles.map((role) => (
                    <article key={role.id} className="mini-card">
                      <strong>{role.name}</strong>
                      <span className="status-chip neutral">
                        {prettyStatus(role.default_executor_kind || 'shell')}
                      </span>
                      <p>{role.description || role.system_prompt}</p>
                    </article>
                  ))}
                </div>
              </section>
            )}

            {(view === 'overview' || view === 'skills') && (
              <section className="panel">
                <div className="panel-header">
                  <div>
                    <p className="eyebrow">Skills</p>
                    <h3>Attach reusable guidance to roles</h3>
                  </div>
                  <span className="status-chip neutral">{dashboard.skills.length} skills</span>
                </div>

                <div className="form-grid">
                  <input
                    value={skillForm.name}
                    onChange={(event) =>
                      setSkillForm((current) => ({ ...current, name: event.target.value }))
                    }
                    placeholder="Skill name"
                  />
                  <select
                    value={skillForm.source}
                    onChange={(event) =>
                      setSkillForm((current) => ({
                        ...current,
                        source: event.target.value as SkillDefinition['source'],
                      }))
                    }
                  >
                    <option value="inline">Inline</option>
                    <option value="agents_md">AGENTS.md</option>
                    <option value="file">File</option>
                    <option value="remote">Remote</option>
                  </select>
                  <input
                    value={skillForm.description}
                    onChange={(event) =>
                      setSkillForm((current) => ({
                        ...current,
                        description: event.target.value,
                      }))
                    }
                    placeholder="Skill description"
                  />
                </div>
                <textarea
                  value={skillForm.instructions}
                  onChange={(event) =>
                    setSkillForm((current) => ({
                      ...current,
                      instructions: event.target.value,
                    }))
                  }
                  placeholder="Skill instructions"
                />
                <button
                  className="primary-button"
                  disabled={!skillForm.name || !skillForm.instructions || submitting !== null}
                  onClick={() => void createSkill()}
                >
                  {submitting === 'create-skill' ? 'Creating…' : 'Create skill'}
                </button>

                <div className="form-grid">
                  <select
                    value={roleSkillForm.roleId}
                    onChange={(event) =>
                      setRoleSkillForm((current) => ({ ...current, roleId: event.target.value }))
                    }
                  >
                    <option value="">Select role</option>
                    {dashboard.roles.map((role) => (
                      <option key={role.id} value={role.id}>
                        {role.name}
                      </option>
                    ))}
                  </select>
                  <select
                    value={roleSkillForm.skillId}
                    onChange={(event) =>
                      setRoleSkillForm((current) => ({ ...current, skillId: event.target.value }))
                    }
                  >
                    <option value="">Select skill</option>
                    {dashboard.skills.map((skill) => (
                      <option key={skill.id} value={skill.id}>
                        {skill.name}
                      </option>
                    ))}
                  </select>
                  <button
                    className="ghost-button"
                    disabled={!roleSkillForm.roleId || !roleSkillForm.skillId || submitting !== null}
                    onClick={() => void bindSkillToRole()}
                  >
                    {submitting === 'bind-skill' ? 'Attaching…' : 'Attach skill to role'}
                  </button>
                </div>

                <div className="card-grid">
                  {dashboard.skills.map((skill) => (
                    <article key={skill.id} className="mini-card">
                      <strong>{skill.name}</strong>
                      <span className="status-chip neutral">{prettyStatus(skill.source)}</span>
                      <p>{skill.instructions}</p>
                    </article>
                  ))}
                </div>
              </section>
            )}

            {(view === 'overview' || view === 'goals') && (
              <section className="panel">
                <div className="panel-header">
                  <div>
                    <p className="eyebrow">Goals</p>
                    <h3>Create app goals and compile them into workflows</h3>
                  </div>
                  <span className="status-chip neutral">{dashboard.goals.length} goals</span>
                </div>

                <div className="form-grid">
                  <select
                    value={goalForm.projectId}
                    onChange={(event) =>
                      setGoalForm((current) => ({ ...current, projectId: event.target.value }))
                    }
                  >
                    <option value="">Select project</option>
                    {dashboard.projects.map((project) => (
                      <option key={project.id} value={project.id}>
                        {project.name}
                      </option>
                    ))}
                  </select>
                  <select
                    value={goalForm.kind}
                    onChange={(event) =>
                      setGoalForm((current) => ({
                        ...current,
                        kind: event.target.value as GoalSpec['kind'],
                      }))
                    }
                  >
                    <option value="create_app">Create app</option>
                    <option value="create_workflow">Create workflow</option>
                  </select>
                  <input
                    value={goalForm.title}
                    onChange={(event) =>
                      setGoalForm((current) => ({ ...current, title: event.target.value }))
                    }
                    placeholder="Goal title"
                  />
                </div>
                <textarea
                  value={goalForm.prompt}
                  onChange={(event) =>
                    setGoalForm((current) => ({ ...current, prompt: event.target.value }))
                  }
                  placeholder="Describe what the super owner should produce"
                />
                <button
                  className="primary-button"
                  disabled={!goalForm.projectId || !goalForm.title || !goalForm.prompt || submitting !== null}
                  onClick={() => void createGoal()}
                >
                  {submitting === 'create-goal' ? 'Creating…' : 'Create goal'}
                </button>

                <div className="table-list">
                  {dashboard.goals.map((goal) => (
                    <article key={goal.id} className="list-row stacked">
                      <div>
                        <strong>{goal.title}</strong>
                        <p>{goal.prompt}</p>
                      </div>
                      <div className="row-tags">
                        <span className="status-chip neutral">{prettyStatus(goal.kind)}</span>
                        <span className={`status-chip ${goal.status}`}>{prettyStatus(goal.status)}</span>
                      </div>
                      <button
                        className="ghost-button"
                        disabled={submitting !== null}
                        onClick={() => void compileGoal(goal.id)}
                      >
                        {submitting === `compile-goal-${goal.id}` ? 'Compiling…' : 'Compile goal'}
                      </button>
                    </article>
                  ))}
                </div>

                {compiledGoal && (
                  <article className="mini-card">
                    <strong>Latest compiled workflow: {compiledGoal.workflow.name}</strong>
                    <p>{compiledGoal.workflow.description}</p>
                    <p>{compiledGoal.workflow.steps.length} generated steps</p>
                  </article>
                )}
              </section>
            )}

            {(view === 'overview' || view === 'workflows') && (
              <section className="panel">
                <div className="panel-header">
                  <div>
                    <p className="eyebrow">Workflows</p>
                    <h3>Template repeatable playbooks</h3>
                  </div>
                  <span className="status-chip neutral">{dashboard.workflows.length} templates</span>
                </div>

                <div className="form-grid">
                  <select
                    value={workflowForm.projectId}
                    onChange={(event) =>
                      setWorkflowForm((current) => ({ ...current, projectId: event.target.value }))
                    }
                  >
                    <option value="">Select project</option>
                    {dashboard.projects.map((project) => (
                      <option key={project.id} value={project.id}>
                        {project.name}
                      </option>
                    ))}
                  </select>
                  <input
                    value={workflowForm.name}
                    onChange={(event) =>
                      setWorkflowForm((current) => ({ ...current, name: event.target.value }))
                    }
                    placeholder="Workflow name"
                  />
                  <input
                    value={workflowForm.stepName}
                    onChange={(event) =>
                      setWorkflowForm((current) => ({ ...current, stepName: event.target.value }))
                    }
                    placeholder="Initial step name"
                  />
                  <select
                    value={workflowForm.executorKind}
                    onChange={(event) =>
                      setWorkflowForm((current) => ({
                        ...current,
                        executorKind: event.target.value as ExecutorKind,
                      }))
                    }
                  >
                    <option value="native_cli_ai">native-cli-ai</option>
                    <option value="claude_code">Claude Code</option>
                    <option value="codex">Codex CLI</option>
                    <option value="opencode">OpenCode</option>
                    <option value="shell">Shell</option>
                  </select>
                  <select
                    value={workflowForm.roleId}
                    onChange={(event) =>
                      setWorkflowForm((current) => ({ ...current, roleId: event.target.value }))
                    }
                  >
                    <option value="">Select role</option>
                    {dashboard.roles.map((role) => (
                      <option key={role.id} value={role.id}>
                        {role.name}
                      </option>
                    ))}
                  </select>
                </div>
                <textarea
                  value={workflowForm.description}
                  onChange={(event) =>
                    setWorkflowForm((current) => ({
                      ...current,
                      description: event.target.value,
                    }))
                  }
                  placeholder="Workflow description"
                />
                <textarea
                  value={workflowForm.stepInstruction}
                  onChange={(event) =>
                    setWorkflowForm((current) => ({
                      ...current,
                      stepInstruction: event.target.value,
                    }))
                  }
                  placeholder="Step instruction"
                />
                <label className="toggle-row">
                  <input
                    type="checkbox"
                    checked={workflowForm.requiresApproval}
                    onChange={(event) =>
                      setWorkflowForm((current) => ({
                        ...current,
                        requiresApproval: event.target.checked,
                      }))
                    }
                  />
                  Require operator approval before execution
                </label>

                <button
                  className="primary-button"
                  disabled={!workflowForm.name || !workflowForm.stepInstruction || submitting !== null}
                  onClick={() => void createWorkflow()}
                >
                  {submitting === 'create-workflow' ? 'Creating…' : 'Create workflow'}
                </button>

                <div className="table-list">
                  {dashboard.workflows.map((workflow) => (
                    <article key={workflow.id} className="list-row stacked">
                      <div>
                        <strong>{workflow.name}</strong>
                        <p>{workflow.description || 'No description provided'}</p>
                      </div>
                      <div className="row-tags">
                        <span className="status-chip neutral">
                          {projectIndex[workflow.project_id || '']?.name || 'Shared'}
                        </span>
                        <span className="status-chip neutral">
                          {workflow.steps.length} step{workflow.steps.length === 1 ? '' : 's'}
                        </span>
                        {workflow.steps[0]?.role_id ? (
                          <span className="status-chip neutral">
                            {roleIndex[workflow.steps[0].role_id]?.name || 'Role assigned'}
                          </span>
                        ) : null}
                      </div>
                    </article>
                  ))}
                  {!dashboard.workflows.length && (
                    <div className="empty-state">No workflow templates available yet.</div>
                  )}
                </div>
              </section>
            )}

            {(view === 'overview' || view === 'runs') && (
              <section className="panel">
                <div className="panel-header">
                  <div>
                    <p className="eyebrow">Runs</p>
                    <h3>Launch and monitor executions</h3>
                  </div>
                  <span className="status-chip neutral">{dashboard.runs.length} runs</span>
                </div>

                <div className="form-grid">
                  <select
                    value={runForm.projectId}
                    onChange={(event) =>
                      setRunForm((current) => ({ ...current, projectId: event.target.value }))
                    }
                  >
                    <option value="">Select project</option>
                    {dashboard.projects.map((project) => (
                      <option key={project.id} value={project.id}>
                        {project.name}
                      </option>
                    ))}
                  </select>
                  <select
                    value={runForm.workflowTemplateId}
                    onChange={(event) =>
                      setRunForm((current) => ({
                        ...current,
                        workflowTemplateId: event.target.value,
                      }))
                    }
                  >
                    <option value="">Select workflow</option>
                    {dashboard.workflows.map((workflow) => (
                      <option key={workflow.id} value={workflow.id}>
                        {workflow.name}
                      </option>
                    ))}
                  </select>
                  <select
                    value={runForm.executorProfileId}
                    onChange={(event) =>
                      setRunForm((current) => ({
                        ...current,
                        executorProfileId: event.target.value,
                      }))
                    }
                  >
                    <option value="">Select executor</option>
                    {dashboard.executors.map((executor) => (
                      <option key={executor.id} value={executor.id}>
                        {executor.name}
                      </option>
                    ))}
                  </select>
                  <input
                    value={runForm.requestedBy}
                    onChange={(event) =>
                      setRunForm((current) => ({
                        ...current,
                        requestedBy: event.target.value,
                      }))
                    }
                    placeholder="Requested by"
                  />
                </div>

                <button
                  className="primary-button"
                  disabled={
                    !runForm.projectId || !runForm.workflowTemplateId || submitting !== null
                  }
                  onClick={() => void createRun()}
                >
                  {submitting === 'create-run' ? 'Launching…' : 'Launch run'}
                </button>

                <div className="table-list">
                  {dashboard.runs.map((run) => (
                    <button
                      key={run.id}
                      className={`list-row interactive ${selectedRunId === run.id ? 'selected' : ''}`}
                      onClick={() => setSelectedRunId(run.id)}
                    >
                      <div>
                        <strong>{workflowIndex[run.workflow_template_id]?.name || 'Workflow'}</strong>
                        <p>{projectIndex[run.project_id]?.name || 'Unknown project'}</p>
                      </div>
                      <span className={`status-chip ${run.status}`}>{prettyStatus(run.status)}</span>
                    </button>
                  ))}
                  {!dashboard.runs.length && (
                    <div className="empty-state">No runs yet. Launch the first workflow above.</div>
                  )}
                </div>
              </section>
            )}
          </div>

          <div className="secondary-column">
            <section className="panel sticky-panel">
              <div className="panel-header">
                <div>
                  <p className="eyebrow">Run detail</p>
                  <h3>{selectedRun ? `Run ${selectedRun.run.id.slice(0, 8)}` : 'Select a run'}</h3>
                </div>
                {selectedRun ? (
                  <span className={`status-chip ${selectedRun.run.status}`}>
                    {prettyStatus(selectedRun.run.status)}
                  </span>
                ) : null}
              </div>

              {selectedRun ? (
                <>
                  <div className="detail-meta">
                    <div>
                      <span className="meta-label">Project</span>
                      <strong>{projectIndex[selectedRun.run.project_id]?.name || 'Unknown'}</strong>
                    </div>
                    <div>
                      <span className="meta-label">Workflow</span>
                      <strong>
                        {workflowIndex[selectedRun.run.workflow_template_id]?.name || 'Unknown'}
                      </strong>
                    </div>
                    <div>
                      <span className="meta-label">Executor</span>
                      <strong>
                        {executorIndex[selectedRun.run.executor_profile_id || '']?.name ||
                          'Unassigned'}
                      </strong>
                    </div>
                  </div>

                  {selectedRun.pending_approval ? (
                    <div className="approval-card">
                      <p className="eyebrow">Approval required</p>
                      <strong>{activeWorkflowStep?.name || 'Pending step'}</strong>
                      <p>{activeWorkflowStep?.instruction}</p>
                      <div className="button-row">
                        <button
                          className="primary-button"
                          disabled={submitting !== null}
                          onClick={() => void resolveApproval('approve')}
                        >
                          {submitting === 'approve-approval' ? 'Approving…' : 'Approve'}
                        </button>
                        <button
                          className="danger-button"
                          disabled={submitting !== null}
                          onClick={() => void resolveApproval('reject')}
                        >
                          {submitting === 'reject-approval' ? 'Rejecting…' : 'Reject'}
                        </button>
                      </div>
                    </div>
                  ) : null}

                  <div className="step-list">
                    {selectedRun.run_steps.map((runStep) => {
                      const workflowStep = selectedRun.workflow_steps.find(
                        (step) => step.id === runStep.workflow_step_id,
                      )

                      return (
                        <article key={runStep.id} className="step-card">
                          <div className="step-header">
                            <div>
                              <strong>{workflowStep?.name || 'Workflow step'}</strong>
                              <p>{workflowStep?.instruction}</p>
                            </div>
                            <span className={`status-chip ${runStep.status}`}>
                              {prettyStatus(runStep.status)}
                            </span>
                          </div>

                          <div className="row-tags">
                            <span className="status-chip neutral">
                              {prettyStatus(workflowStep?.executor_kind || 'shell')}
                            </span>
                            {workflowStep?.role_id ? (
                              <span className="status-chip neutral">
                                {roleIndex[workflowStep.role_id]?.name || 'Assigned role'}
                              </span>
                            ) : null}
                            {workflowStep?.requires_approval ? (
                              <span className="status-chip warning">approval gate</span>
                            ) : null}
                          </div>

                          {runStep.status === 'running' ? (
                            <button
                              className="ghost-button"
                              disabled={submitting !== null}
                              onClick={() => void completeStep(runStep.id)}
                            >
                              {submitting === `complete-step-${runStep.id}`
                                ? 'Completing…'
                                : 'Mark step complete'}
                            </button>
                          ) : null}
                        </article>
                      )
                    })}
                  </div>

                  <div className="artifact-section">
                    <div className="panel-header compact">
                      <div>
                        <p className="eyebrow">Artifacts</p>
                        <h3>Run outputs</h3>
                      </div>
                    </div>
                    <div className="card-grid">
                      {selectedArtifacts.map((artifact) => (
                        <article key={artifact.id} className="mini-card">
                          <strong>{artifact.name}</strong>
                          <span className="status-chip neutral">{artifact.kind}</span>
                          <p>{JSON.stringify(artifact.metadata_json)}</p>
                        </article>
                      ))}
                      {!selectedArtifacts.length && (
                        <div className="empty-state">
                          Artifacts will appear here after the run completes.
                        </div>
                      )}
                    </div>
                  </div>
                </>
              ) : (
                <div className="empty-state">
                  Choose a run to inspect step status, approvals, and operator actions.
                </div>
              )}
            </section>

            <section className="panel">
              <div className="panel-header">
                <div>
                  <p className="eyebrow">Phone control</p>
                  <h3>Pair a remote browser session</h3>
                </div>
                <span className="status-chip warning">
                  {dashboard.pairings.filter((session) => !session.is_revoked).length} active
                </span>
              </div>

              <div className="form-grid">
                <input
                  value={pairingForm.label}
                  onChange={(event) =>
                    setPairingForm((current) => ({ ...current, label: event.target.value }))
                  }
                  placeholder="Pairing label"
                />
                <input
                  type="number"
                  min={5}
                  value={pairingForm.expiresInMinutes}
                  onChange={(event) =>
                    setPairingForm((current) => ({
                      ...current,
                      expiresInMinutes: Number(event.target.value) || 60,
                    }))
                  }
                  placeholder="Expires in minutes"
                />
              </div>

              <button
                className="primary-button"
                disabled={submitting !== null}
                onClick={() => void createPairingSession()}
              >
                {submitting === 'create-pairing' ? 'Generating…' : 'Generate QR pairing'}
              </button>

              {activePairingUrl ? (
                <div className="pairing-card">
                  {pairingQrDataUrl ? (
                    <img
                      src={pairingQrDataUrl}
                      alt="QR code for remote phone control"
                      className="qr-code"
                    />
                  ) : null}
                  <div className="pairing-copy">
                    <span className="meta-label">Remote URL</span>
                    <code>{activePairingUrl}</code>
                  </div>
                </div>
              ) : null}

              <div className="table-list">
                {dashboard.pairings.map((session) => (
                  <article key={session.id} className="list-row stacked">
                    <div>
                      <strong>{session.label || 'Remote session'}</strong>
                      <p>{session.token}</p>
                    </div>
                    <div className="row-tags">
                      <span className={`status-chip ${session.is_revoked ? 'cancelled' : 'healthy'}`}>
                        {session.is_revoked ? 'revoked' : 'active'}
                      </span>
                      <span className="status-chip neutral">
                        {session.expires_at
                          ? `expires ${new Date(session.expires_at).toLocaleTimeString()}`
                          : 'no expiry'}
                      </span>
                    </div>
                    {!session.is_revoked ? (
                      <button
                        className="ghost-button"
                        disabled={submitting !== null}
                        onClick={() => void revokePairingSession(session.id)}
                      >
                        {submitting === `revoke-pairing-${session.id}` ? 'Revoking…' : 'Revoke'}
                      </button>
                    ) : null}
                  </article>
                ))}
                {!dashboard.pairings.length && (
                  <div className="empty-state">
                    No remote pairings yet. Generate a QR code to open the dashboard from your phone.
                  </div>
                )}
              </div>
            </section>

            <section className="panel">
              <div className="panel-header">
                <div>
                  <p className="eyebrow">Event timeline</p>
                  <h3>Recent orchestration activity</h3>
                </div>
              </div>
              <div className="event-list">
                {dashboard.events.slice(0, 10).map((event) => (
                  <article key={event.id} className="event-item">
                    <div>
                      <strong>{event.summary}</strong>
                      <p>{event.event_type}</p>
                    </div>
                    <time>{new Date(event.created_at).toLocaleTimeString()}</time>
                  </article>
                ))}
                {!dashboard.events.length && (
                  <div className="empty-state">No events yet. Actions will appear here live.</div>
                )}
              </div>
            </section>
          </div>
        </section>
      </main>
    </div>
  )
}

export default App
