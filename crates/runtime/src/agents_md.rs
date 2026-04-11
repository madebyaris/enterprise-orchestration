use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentsDocument {
    pub path: String,
    pub instructions: String,
    pub sections: Vec<AgentsSection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentsSection {
    pub heading: String,
    pub content: String,
}

pub fn load_agents_document(path: &Path) -> Result<AgentsDocument> {
    let instructions = fs::read_to_string(path)
        .with_context(|| format!("failed to read AGENTS.md at {}", path.display()))?;
    Ok(AgentsDocument {
        path: path.to_string_lossy().into_owned(),
        sections: parse_sections(&instructions),
        instructions,
    })
}

fn parse_sections(markdown: &str) -> Vec<AgentsSection> {
    let mut sections = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_content = String::new();

    for line in markdown.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if let Some(current_heading) = current_heading.take() {
                sections.push(AgentsSection {
                    heading: current_heading,
                    content: current_content.trim().to_string(),
                });
                current_content.clear();
            }
            current_heading = Some(heading.trim().to_string());
        } else {
            if !current_content.is_empty() {
                current_content.push('\n');
            }
            current_content.push_str(line);
        }
    }

    if let Some(current_heading) = current_heading {
        sections.push(AgentsSection {
            heading: current_heading,
            content: current_content.trim().to_string(),
        });
    }

    sections
}

#[cfg(test)]
mod tests {
    use super::parse_sections;

    #[test]
    fn parses_heading_sections() {
        let sections = parse_sections(
            "# AGENTS.md\n\n## Setup\nRun tests.\n\n## Style\nUse Rust fmt.\n",
        );
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, "Setup");
        assert!(sections[0].content.contains("Run tests."));
    }
}
