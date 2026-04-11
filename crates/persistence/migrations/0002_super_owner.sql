ALTER TABLE projects ADD COLUMN agents_md_path TEXT;
ALTER TABLE projects ADD COLUMN agents_md_updated_at TEXT;

ALTER TABLE workflow_steps ADD COLUMN role_id TEXT;
ALTER TABLE workflow_steps ADD COLUMN success_criteria TEXT;
ALTER TABLE workflow_steps ADD COLUMN artifact_contract TEXT;
ALTER TABLE workflow_steps ADD COLUMN input_schema_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE workflow_steps ADD COLUMN output_schema_json TEXT NOT NULL DEFAULT '{}';

ALTER TABLE runs ADD COLUMN goal_id TEXT;
ALTER TABLE runs ADD COLUMN compiled_by TEXT;
ALTER TABLE runs ADD COLUMN assigned_role_id TEXT;
ALTER TABLE runs ADD COLUMN effective_executor_kind TEXT;

CREATE TABLE IF NOT EXISTS agent_roles (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT,
  system_prompt TEXT NOT NULL,
  default_executor_kind TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS skills (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT,
  instructions TEXT NOT NULL,
  source TEXT NOT NULL,
  source_uri TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS role_skills (
  role_id TEXT NOT NULL,
  skill_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY(role_id, skill_id),
  FOREIGN KEY(role_id) REFERENCES agent_roles(id) ON DELETE CASCADE,
  FOREIGN KEY(skill_id) REFERENCES skills(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS goals (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  title TEXT NOT NULL,
  prompt TEXT NOT NULL,
  status TEXT NOT NULL,
  compiled_workflow_template_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
  FOREIGN KEY(compiled_workflow_template_id) REFERENCES workflow_templates(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS organization_templates (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
