use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::agents_md::{load_agents_document, AgentsDocument};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectContextSnapshot {
    pub workspace_path: String,
    pub root_agents_md: Option<AgentsDocument>,
    pub nested_agents_md: Vec<AgentsDocument>,
    pub discovered_at: DateTime<Utc>,
}

#[derive(Default, Clone)]
pub struct ProjectContextService;

impl ProjectContextService {
    pub fn discover(&self, workspace_path: impl AsRef<Path>) -> Result<ProjectContextSnapshot> {
        let workspace_path = workspace_path.as_ref();
        let root_agents_md_path = workspace_path.join("AGENTS.md");
        let root_agents_md = if root_agents_md_path.exists() {
            Some(load_agents_document(&root_agents_md_path)?)
        } else {
            None
        };

        let mut nested_paths = Vec::new();
        collect_agents_paths(workspace_path, workspace_path, &mut nested_paths)?;
        nested_paths.retain(|path| path != &root_agents_md_path);

        let mut nested_agents_md = Vec::new();
        for path in nested_paths {
            nested_agents_md.push(load_agents_document(&path)?);
        }

        Ok(ProjectContextSnapshot {
            workspace_path: workspace_path.to_string_lossy().into_owned(),
            root_agents_md,
            nested_agents_md,
            discovered_at: Utc::now(),
        })
    }

    pub fn nearest_agents_document<'a>(
        &self,
        snapshot: &'a ProjectContextSnapshot,
        target_path: impl AsRef<Path>,
    ) -> Option<&'a AgentsDocument> {
        let target_path = target_path.as_ref();
        let mut candidates = snapshot.nested_agents_md.iter().collect::<Vec<_>>();
        if let Some(root) = snapshot.root_agents_md.as_ref() {
            candidates.push(root);
        }

        candidates
            .into_iter()
            .filter(|document| {
                let document_parent = Path::new(&document.path).parent().unwrap_or_else(|| Path::new(""));
                target_path.starts_with(document_parent)
            })
            .max_by_key(|document| document.path.len())
    }
}

fn collect_agents_paths(
    root: &Path,
    current: &Path,
    output: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(current)
        .with_context(|| format!("failed to read directory {}", current.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == ".git" || name == "target" || name == "node_modules" {
                continue;
            }
            collect_agents_paths(root, &path, output)?;
        } else if file_type.is_file() && path.file_name().and_then(|name| name.to_str()) == Some("AGENTS.md")
        {
            if path.starts_with(root) {
                output.push(path);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};

    use super::ProjectContextService;

    #[test]
    fn discovers_root_and_nested_agents_files() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("runtime-context-{unique}"));
        let nested = dir.join("apps/control");
        fs::create_dir_all(&nested).expect("dirs");
        fs::write(dir.join("AGENTS.md"), "## Setup\nroot").expect("root");
        fs::write(nested.join("AGENTS.md"), "## Setup\nnested").expect("nested");

        let service = ProjectContextService;
        let snapshot = service.discover(&dir).expect("snapshot");
        let nearest = service
            .nearest_agents_document(&snapshot, nested.join("src/main.rs"))
            .expect("nearest");

        assert!(snapshot.root_agents_md.is_some());
        assert_eq!(snapshot.nested_agents_md.len(), 1);
        assert!(nearest.instructions.contains("nested"));

        let _ = fs::remove_dir_all(&dir);
    }
}
