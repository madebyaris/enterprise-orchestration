use domain::{AgentRole, GoalSpec, Project, SkillDefinition};

pub struct PromptBuilder;

impl PromptBuilder {
    pub fn build_step_prompt(
        role: &RolePromptContext<'_>,
        goal: &GoalSpec,
        project: &Project,
        step_instructions: &str,
        agents_md: Option<&str>,
    ) -> String {
        let mut prompt = String::new();
        prompt.push_str("You are operating inside Enterprise Orchestration.\n");
        prompt.push_str(&format!("Role: {}\n", role.name));
        if let Some(description) = role.description {
            prompt.push_str(&format!("Role description: {description}\n"));
        }
        prompt.push_str(&format!("Project: {}\n", project.name));
        prompt.push_str(&format!("Workspace: {}\n", project.workspace_path));
        prompt.push_str(&format!("Goal: {}\n", goal.title));
        prompt.push_str(&format!("Goal prompt: {}\n", goal.prompt));
        prompt.push_str("\nRole system prompt:\n");
        prompt.push_str(role.system_prompt);
        prompt.push_str("\n\nStep instructions:\n");
        prompt.push_str(step_instructions);

        if !role.skills.is_empty() {
            prompt.push_str("\n\nAttached skills:\n");
            for skill in role.skills {
                prompt.push_str(&format!("- {}: {}\n", skill.name, skill.instructions));
            }
        }

        if let Some(agents_md) = agents_md {
            prompt.push_str("\n\nProject guidance from AGENTS.md:\n");
            prompt.push_str(agents_md);
        }

        prompt
    }
}

pub struct RolePromptContext<'a> {
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub system_prompt: &'a str,
    pub skills: &'a [SkillDefinition],
}

impl<'a> From<(&'a AgentRole, &'a [SkillDefinition])> for RolePromptContext<'a> {
    fn from(value: (&'a AgentRole, &'a [SkillDefinition])) -> Self {
        Self {
            name: &value.0.name,
            description: value.0.description.as_deref(),
            system_prompt: &value.0.system_prompt,
            skills: value.1,
        }
    }
}
