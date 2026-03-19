import { useCallback, useEffect, useMemo, useState } from 'react'
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
}

type ExecutorProfile = {
  id: string
  name: string
  kind: ExecutorKind
  binary_path?: string | null
}

type WorkflowStep = {
  id: string
  workflow_template_id: string
  name: string
  instruction: string
  order_index: number
  executor_kind: ExecutorKind
  requires_approval: boolean
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
  executors: ExecutorProfile[]
  workflows: WorkflowTemplate[]
  runs: Run[]
  approvals: ApprovalGate[]
  events: EventEnvelope[]
}

type ViewKey = 'overview' | 'projects' | 'executors' | 'workflows' | 'runs'

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
  const response = await fetch(`${API_BASE}${path}`, {
    headers: {
      'Content-Type': 'application/json',
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
    executors: [],
    workflows: [],
    runs: [],
    approvals: [],
    events: [],
  })
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null)
  const [selectedRun, setSelectedRun] = useState<RunSnapshot | null>(null)
  const [loading, setLoading] = useState(true)
  const [submitting, setSubmitting] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [successMessage, setSuccessMessage] = useState<string | null>(null)
  const [lastRefreshedAt, setLastRefreshedAt] = useState<Date | null>(null)

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
  const [workflowForm, setWorkflowForm] = useState({
    projectId: '',
    name: 'Repository audit',
    description: 'Inspect the repository and create an operator-ready plan.',
    stepName: 'Inspect repository',
    stepInstruction: 'Inspect the repository and create an operator-ready plan.',
    executorKind: 'native_cli_ai' as ExecutorKind,
    requiresApproval: true,
  })
  const [runForm, setRunForm] = useState({
    projectId: '',
    workflowTemplateId: '',
    executorProfileId: '',
    requestedBy: 'operator',
  })

  const refreshDashboard = useCallback(async () => {
    const [projects, executors, workflows, runs, approvals, events] = await Promise.all([
      requestJson<Project[]>('/api/projects'),
      requestJson<ExecutorProfile[]>('/api/executors'),
      requestJson<WorkflowTemplate[]>('/api/workflows'),
      requestJson<Run[]>('/api/runs'),
      requestJson<ApprovalGate[]>('/api/approvals'),
      requestJson<EventEnvelope[]>('/api/events'),
    ])

    setDashboard({ projects, executors, workflows, runs, approvals, events })
    setLastRefreshedAt(new Date())

    if (!selectedRunId && runs[0]) {
      setSelectedRunId(runs[0].id)
    }
  }, [selectedRunId])

  const refreshSelectedRun = useCallback(async () => {
    if (!selectedRunId) {
      setSelectedRun(null)
      return
    }

    try {
      const snapshot = await requestJson<RunSnapshot>(`/api/runs/${selectedRunId}`)
      setSelectedRun(snapshot)
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
  }, [dashboard, runForm.executorProfileId, runForm.projectId, runForm.workflowTemplateId, workflowForm.projectId])

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
              depends_on_step_id: null,
              timeout_seconds: 300,
              retry_limit: 1,
              requires_approval: workflowForm.requiresApproval,
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
      setView('runs')
      setSuccessMessage(`Started run ${snapshot.run.id.slice(0, 8)}`)
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
          {(['overview', 'projects', 'executors', 'workflows', 'runs'] as ViewKey[]).map((item) => (
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
                    <article key={project.id} className="list-row">
                      <div>
                        <strong>{project.name}</strong>
                        <p>{project.workspace_path}</p>
                      </div>
                      <span className="muted">{project.repository_url || 'local-only'}</span>
                    </article>
                  ))}
                  {!dashboard.projects.length && (
                    <div className="empty-state">No projects yet. Create one to get started.</div>
                  )}
                </div>
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
                    </article>
                  ))}
                  {!dashboard.executors.length && (
                    <div className="empty-state">No executor profiles configured yet.</div>
                  )}
                </div>
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
