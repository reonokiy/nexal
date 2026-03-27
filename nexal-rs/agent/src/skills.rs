//! Skill loading — scans SKILL.md files and builds a system prompt section.
//!
//! Ports the Python `_load_skill_docs` function from `nexal/bots/agent.py`.
//!
//! Skills are directories under the workspace `skills/` directory, each containing
//! a `SKILL.md` file with optional YAML frontmatter.
//!
//! Skills matching active channel names are always loaded. Skills with
//! `always_load: true` in their frontmatter are loaded regardless of channel.

use std::path::Path;

use tracing::{debug, warn};

/// Container-side path where skills are mounted.
const CONTAINER_SKILLS_DIR: &str = "/workspace/agents/skills";

/// Load skill docs for the given channel names.
///
/// Returns a combined string of all matching SKILL.md contents,
/// with paths rewritten to container-side mount points.
pub async fn load_skill_docs(skills_dir: &Path, channel_names: &[&str]) -> String {
    let mut skill_names: Vec<String> = channel_names.iter().map(|s| s.to_string()).collect();

    // Scan for always_load skills
    if let Ok(mut entries) = tokio::fs::read_dir(skills_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if !entry.path().is_dir() || skill_names.contains(&name) {
                continue;
            }

            let skill_md = entry.path().join("SKILL.md");
            if let Ok(raw) = tokio::fs::read_to_string(&skill_md).await {
                if is_always_load(&raw) {
                    debug!(skill = %name, "auto-loading always_load skill");
                    skill_names.push(name);
                }
            }
        }
    }

    // Load content for each skill
    let mut parts = Vec::new();
    for name in &skill_names {
        let skill_md = skills_dir.join(name).join("SKILL.md");
        match tokio::fs::read_to_string(&skill_md).await {
            Ok(content) => {
                let content = strip_frontmatter(&content);
                // Rewrite relative script paths to container-side paths
                let content =
                    content.replace("./scripts/", &format!("{CONTAINER_SKILLS_DIR}/{name}/scripts/"));
                parts.push(content);
            }
            Err(_) => {
                debug!(skill = %name, "SKILL.md not found, skipping");
            }
        }
    }

    if parts.is_empty() {
        "(no channel skills available)".to_string()
    } else {
        parts.join("\n\n")
    }
}

/// Check if a SKILL.md has `always_load: true` in its frontmatter.
fn is_always_load(raw: &str) -> bool {
    if !raw.starts_with("---") {
        return false;
    }
    let Some(end) = raw[3..].find("---") else {
        return false;
    };
    let frontmatter = &raw[3..3 + end];
    frontmatter.contains("always_load: true")
}

/// Strip YAML frontmatter (between `---` delimiters) from the content.
fn strip_frontmatter(content: &str) -> String {
    if !content.starts_with("---") {
        return content.to_string();
    }
    if let Some(end) = content[3..].find("---") {
        content[3 + end + 3..].trim().to_string()
    } else {
        content.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_frontmatter() {
        let input = "---\nname: test\nalways_load: true\n---\n# Skill Title\nContent here.";
        assert_eq!(strip_frontmatter(input), "# Skill Title\nContent here.");
    }

    #[test]
    fn test_strip_frontmatter_no_front() {
        let input = "# Just content\nNo frontmatter.";
        assert_eq!(strip_frontmatter(input), input);
    }

    #[test]
    fn test_is_always_load() {
        assert!(is_always_load("---\nalways_load: true\n---\ncontent"));
        assert!(!is_always_load("---\nalways_load: false\n---\ncontent"));
        assert!(!is_always_load("no frontmatter"));
    }
}
