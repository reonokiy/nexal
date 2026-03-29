//! Skill loading — two directories, override takes priority:
//!
//! 1. Built-in skills (`skills/`) — read-only, shipped with nexal
//! 2. Override skills (`skills.override/`) — read-write, agent-created
//!
//! Override skills with the same name replace built-in ones.

use std::collections::HashMap;
use std::path::Path;

use tracing::debug;

/// Container-side paths.
const BUILTIN_SKILLS_DIR: &str = "/workspace/agents/skills";
const OVERRIDE_SKILLS_DIR: &str = "/workspace/agents/skills.override";

/// Load skill docs from both built-in and override directories.
pub async fn load_skill_docs(
    builtin_dir: &Path,
    override_dir: &Path,
    channel_names: &[&str],
) -> String {
    let mut skills: HashMap<String, SkillEntry> = HashMap::new();

    // Built-in first, then override replaces same-name
    scan_dir(builtin_dir, BUILTIN_SKILLS_DIR, &mut skills).await;
    scan_dir(override_dir, OVERRIDE_SKILLS_DIR, &mut skills).await;

    let channels: Vec<String> = channel_names.iter().map(|s| s.to_string()).collect();
    let mut parts = Vec::new();

    for (name, entry) in &skills {
        if entry.always_load || channels.contains(name) {
            parts.push(entry.content.clone());
        }
    }

    if parts.is_empty() {
        "(no skills available)".to_string()
    } else {
        parts.join("\n\n")
    }
}

struct SkillEntry {
    content: String,
    always_load: bool,
}

async fn scan_dir(dir: &Path, container_dir: &str, skills: &mut HashMap<String, SkillEntry>) {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if !entry.path().is_dir() {
            continue;
        }
        let skill_md = entry.path().join("SKILL.md");
        let Ok(raw) = tokio::fs::read_to_string(&skill_md).await else {
            continue;
        };
        let always_load = is_always_load(&raw);
        let content = strip_frontmatter(&raw)
            .replace("./scripts/", &format!("{container_dir}/{name}/scripts/"));
        debug!(skill = %name, source = %container_dir, always_load, "loaded skill");
        skills.insert(name, SkillEntry { content, always_load });
    }
}

fn is_always_load(raw: &str) -> bool {
    if !raw.starts_with("---") {
        return false;
    }
    let Some(end) = raw[3..].find("---") else {
        return false;
    };
    raw[3..3 + end].contains("always_load: true")
}

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
