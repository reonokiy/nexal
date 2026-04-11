//! Skill loading — two directories, override takes priority:
//!
//! 1. Built-in skills (`skills/`) — read-only, shipped with nexal
//! 2. Override skills (`skills.override/`) — read-write, agent-created
//!
//! Override skills with the same name replace built-in ones.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use tracing::{debug, info};

/// Container-side paths.
const BUILTIN_SKILLS_DIR: &str = "/workspace/agents/skills";
const OVERRIDE_SKILLS_DIR: &str = "/workspace/agents/skills.override";

/// Load skill docs from both built-in and override directories.
///
/// `is_admin`: if false, skills with `admin_only: true` in frontmatter are skipped.
pub async fn load_skill_docs(
    builtin_dir: &Path,
    override_dir: &Path,
    channel_names: &[&str],
    is_admin: bool,
) -> String {
    let mut skills: HashMap<String, SkillEntry> = HashMap::new();

    // Built-in first, then override replaces same-name
    scan_dir(builtin_dir, BUILTIN_SKILLS_DIR, &mut skills).await;
    scan_dir(override_dir, OVERRIDE_SKILLS_DIR, &mut skills).await;

    let channels: Vec<String> = channel_names.iter().map(|s| s.to_string()).collect();
    let mut loaded = Vec::new();
    let mut skipped = Vec::new();
    let mut parts = Vec::new();

    for (name, entry) in &skills {
        // Skip admin-only skills for non-admin users
        if entry.admin_only && !is_admin {
            skipped.push(format!("{name} (admin_only)"));
            continue;
        }
        if entry.always_load || channels.contains(name) {
            loaded.push(name.clone());
            parts.push(entry.content.clone());
        } else {
            skipped.push(name.clone());
        }
    }

    info!(
        scanned = skills.len(),
        loaded = loaded.len(),
        skipped = skipped.len(),
        "skills scan: found={}, loaded=[{}], skipped=[{}]",
        skills.len(),
        loaded.join(", "),
        skipped.join(", "),
    );

    if parts.is_empty() {
        "(no skills available)".to_string()
    } else {
        parts.join("\n\n")
    }
}

struct SkillEntry {
    content: String,
    always_load: bool,
    admin_only: bool,
}

async fn scan_dir(dir: &Path, container_dir: &str, skills: &mut HashMap<String, SkillEntry>) {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Some((name, skill)) = read_skill(&entry, container_dir).await {
            debug!(
                skill = %name,
                source = %container_dir,
                always_load = skill.always_load,
                admin_only = skill.admin_only,
                "loaded skill"
            );
            skills.insert(name, skill);
        }
    }
}

/// Read one skill directory: `<entry>/SKILL.md` with its YAML frontmatter.
/// Returns `None` if the entry is not a directory, has no `SKILL.md`, or
/// any read fails — the outer scan loop just skips it.
async fn read_skill(
    entry: &tokio::fs::DirEntry,
    container_dir: &str,
) -> Option<(String, SkillEntry)> {
    let path = entry.path();
    if !path.is_dir() {
        return None;
    }
    let name = entry.file_name().to_string_lossy().into_owned();
    let raw = tokio::fs::read_to_string(path.join("SKILL.md")).await.ok()?;
    let fm = parse_frontmatter(&raw);
    let content = strip_frontmatter(&raw)
        .replace("./scripts/", &format!("{container_dir}/{name}/scripts/"));
    Some((
        name,
        SkillEntry {
            content,
            always_load: fm.metadata.always_load,
            admin_only: fm.metadata.admin_only,
        },
    ))
}

/// Parsed SKILL.md frontmatter.
#[derive(Debug, Default, Deserialize)]
struct SkillFrontmatter {
    #[serde(default)]
    metadata: SkillMetadataBlock,
}

#[derive(Debug, Default, Deserialize)]
struct SkillMetadataBlock {
    #[serde(default)]
    always_load: bool,
    #[serde(default)]
    admin_only: bool,
}

/// Parse YAML frontmatter from a SKILL.md file.
fn parse_frontmatter(raw: &str) -> SkillFrontmatter {
    if !raw.starts_with("---") {
        return SkillFrontmatter::default();
    }
    let Some(end) = raw[3..].find("---") else {
        return SkillFrontmatter::default();
    };
    let yaml = &raw[3..3 + end];
    serde_yaml::from_str(yaml).unwrap_or_default()
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
    fn test_parse_frontmatter() {
        let fm = parse_frontmatter("---\nmetadata:\n  always_load: true\n  admin_only: true\n---\ncontent");
        assert!(fm.metadata.always_load);
        assert!(fm.metadata.admin_only);

        let fm = parse_frontmatter("---\nname: test\n---\ncontent");
        assert!(!fm.metadata.always_load);
        assert!(!fm.metadata.admin_only);

        let fm = parse_frontmatter("no frontmatter");
        assert!(!fm.metadata.always_load);
    }
}
