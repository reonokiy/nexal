//! Skills service — serves skill files from the host filesystem
//! to in-container agents on demand.
//!
//! Agents request skill metadata and file content through two RPC
//! methods: `skills/list` and `skills/read`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SkillFileEntry {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub files: Vec<SkillFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SkillsListResponse {
    pub skills: Vec<SkillInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SkillsReadParams {
    /// Path relative to the skills root (e.g. "jina-reader/scripts/read.py").
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SkillsReadResponse {
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

pub struct SkillsService {
    skills_dir: PathBuf,
}

impl SkillsService {
    pub fn new(skills_dir: PathBuf) -> Self {
        Self { skills_dir }
    }

    /// List all skills with their file trees.
    pub fn list(&self) -> Result<SkillsListResponse, String> {
        let mut skills = Vec::new();
        let entries =
            std::fs::read_dir(&self.skills_dir).map_err(|e| format!("read skills dir: {e}"))?;

        for entry in entries {
            let entry = entry.map_err(|e| format!("read entry: {e}"))?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "__pycache__" {
                continue;
            }

            let description = read_skill_description(&path);
            let files = collect_files(&path, &path)
                .map_err(|e| format!("collect files for {name}: {e}"))?;

            skills.push(SkillInfo {
                name,
                description,
                files,
            });
        }

        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(SkillsListResponse { skills })
    }

    /// Read a single file by relative path. Validates against path traversal.
    pub fn read(&self, params: SkillsReadParams) -> Result<SkillsReadResponse, String> {
        let rel = sanitize_path(&params.path)?;
        let full = self.skills_dir.join(rel);

        // Ensure the resolved path is still under skills_dir.
        if !full.starts_with(&self.skills_dir) {
            return Err("path traversal".into());
        }

        let data = std::fs::read(&full).map_err(|e| format!("read {}: {e}", params.path))?;
        Ok(SkillsReadResponse { data })
    }
}

/// Sanitize a relative path, rejecting traversal attempts.
fn sanitize_path(p: &str) -> Result<&Path, String> {
    let path = Path::new(p);
    if path.is_absolute() {
        return Err("absolute paths not allowed".into());
    }
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => return Err("path traversal (..)".into()),
            std::path::Component::Normal(_) => {}
            _ => return Err("invalid path component".into()),
        }
    }
    Ok(path)
}

/// Read the first non-empty line from SKILL.md as a description.
fn read_skill_description(skill_dir: &Path) -> String {
    let skill_md = skill_dir.join("SKILL.md");
    match std::fs::read_to_string(&skill_md) {
        Ok(text) => {
            for line in text.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    return trimmed.to_string();
                }
            }
            String::new()
        }
        Err(_) => String::new(),
    }
}

/// Recursively collect files under `dir`, returning paths relative to `root`.
fn collect_files(dir: &Path, root: &Path) -> Result<Vec<SkillFileEntry>, std::io::Error> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name.starts_with('.') || file_name == "__pycache__" {
            continue;
        }
        if path.is_dir() {
            files.extend(collect_files(&path, root)?);
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            files.push(SkillFileEntry { path: rel, size });
        }
    }
    Ok(files)
}
