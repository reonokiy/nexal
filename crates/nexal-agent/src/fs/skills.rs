//! FUSE filesystem for on-demand skill file access.
//!
//! Mounts at `/workspace/agents/skills/` and lazily fetches file
//! content from the gateway via RPC when accessed.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    Request,
};
use tracing::{debug, warn};

use crate::server::channel::dispatch::MsgpackChannel;

const TTL: Duration = Duration::from_secs(60);

/// A node in the virtual filesystem tree.
#[derive(Debug, Clone)]
enum FsNode {
    Dir {
        children: Vec<u64>,
    },
    File {
        /// Relative path for RPC fetch (e.g. "jina-reader/scripts/read.py").
        rel_path: String,
        size: u64,
    },
}

/// Maps inode → node and path → inode.
struct FsTree {
    nodes: HashMap<u64, FsNode>,
    path_to_inode: HashMap<String, u64>,
    next_inode: u64,
}

impl FsTree {
    fn new() -> Self {
        let mut tree = Self {
            nodes: HashMap::new(),
            path_to_inode: HashMap::new(),
            next_inode: 2, // 1 is reserved for root
        };
        // Root directory (inode 1).
        tree.nodes.insert(
            1,
            FsNode::Dir {
                children: Vec::new(),
            },
        );
        tree
    }

    fn root_attr(&self) -> FileAttr {
        FileAttr {
            ino: 1,
            size: 0,
            blocks: 0,
            atime: SystemTime::now(),
            mtime: SystemTime::now(),
            ctime: SystemTime::now(),
            crtime: SystemTime::now(),
            kind: FileType::Directory,
            perm: 0o755,
            nlink: 2,
            uid: 0,
            gid: 0,
            rdev: 0,
            blksize: 512,
            flags: 0,
        }
    }
}

/// Skills FUSE filesystem. Fetches files from the gateway on demand.
pub struct SkillsFuseFs {
    channel: Arc<MsgpackChannel>,
    tree: RwLock<FsTree>,
    file_cache: RwLock<HashMap<String, Vec<u8>>>,
}

impl SkillsFuseFs {
    /// Create and mount the skills filesystem at `mountpoint`.
    /// This blocks the calling thread (runs FUSE in foreground).
    pub(crate) async fn mount(
        channel: Arc<MsgpackChannel>,
        mountpoint: &str,
    ) -> Result<(), String> {
        // Fetch the skill tree from the gateway.
        let resp = channel
            .request("skills/list", rmpv::Value::Nil)
            .await
            .map_err(|e| format!("skills/list failed: {e}"))?;

        let skills: SkillsListResponse =
            rmpv::ext::from_value(resp).map_err(|e| format!("parse skills/list response: {e}"))?;

        let mut tree = FsTree::new();

        // Build the virtual tree from skill metadata.
        let mut root_children = Vec::new();
        for skill in &skills.skills {
            // Create directory for each skill.
            let skill_inode = tree.next_inode;
            tree.next_inode += 1;
            tree.nodes.insert(
                skill_inode,
                FsNode::Dir {
                    children: Vec::new(),
                },
            );

            // Build subdirectories and files.
            for file in &skill.files {
                let parts: Vec<&str> = file.path.split('/').collect();
                let mut parent_inode = skill_inode;
                for (i, _part) in parts.iter().enumerate() {
                    if i == parts.len() - 1 {
                        // File.
                        let file_inode = tree.next_inode;
                        tree.next_inode += 1;
                        let rel_path = format!("{}/{}", skill.name, file.path);
                        tree.nodes.insert(
                            file_inode,
                            FsNode::File {
                                rel_path,
                                size: file.size,
                            },
                        );
                        tree.path_to_inode
                            .insert(format!("{}/{}", skill.name, file.path), file_inode);
                        if let FsNode::Dir { children } = tree.nodes.get_mut(&parent_inode).unwrap()
                        {
                            children.push(file_inode);
                        }
                    } else {
                        // Directory — create if not exists.
                        let dir_path = format!("{}/{}", skill.name, parts[..=i].join("/"));
                        let dir_inode = if let Some(&ino) = tree.path_to_inode.get(&dir_path) {
                            ino
                        } else {
                            let ino = tree.next_inode;
                            tree.next_inode += 1;
                            tree.nodes.insert(
                                ino,
                                FsNode::Dir {
                                    children: Vec::new(),
                                },
                            );
                            tree.path_to_inode.insert(dir_path, ino);
                            if let FsNode::Dir { children } =
                                tree.nodes.get_mut(&parent_inode).unwrap()
                            {
                                children.push(ino);
                            }
                            ino
                        };
                        parent_inode = dir_inode;
                    }
                }
            }

            // Add SKILL.md file.
            let skill_md_inode = tree.next_inode;
            tree.next_inode += 1;
            let rel_path = format!("{}/SKILL.md", skill.name);
            tree.nodes.insert(
                skill_md_inode,
                FsNode::File {
                    rel_path: rel_path.clone(),
                    size: 0, // Will be fetched on read
                },
            );
            tree.path_to_inode.insert(rel_path, skill_md_inode);
            if let FsNode::Dir { children } = tree.nodes.get_mut(&skill_inode).unwrap() {
                children.push(skill_md_inode);
            }
            tree.path_to_inode.insert(skill.name.clone(), skill_inode);
            root_children.push(skill_inode);
        }

        // Update root children.
        if let FsNode::Dir { children } = tree.nodes.get_mut(&1).unwrap() {
            *children = root_children;
        }

        let fs = Self {
            channel,
            tree: RwLock::new(tree),
            file_cache: RwLock::new(HashMap::new()),
        };

        let mountpoint_owned = mountpoint.to_string();
        std::thread::spawn(move || {
            if let Err(e) = fuser::mount2(
                fs,
                &mountpoint_owned,
                &[
                    MountOption::RO,
                    MountOption::AllowOther,
                    MountOption::AutoUnmount,
                ],
            ) {
                warn!("FUSE mount failed at {mountpoint_owned}: {e}");
            }
        });

        debug!("skills FUSE mounted at {mountpoint}");
        Ok(())
    }

    fn file_attr(&self, ino: u64, size: u64) -> FileAttr {
        FileAttr {
            ino,
            size,
            blocks: (size + 511) / 512,
            atime: SystemTime::now(),
            mtime: SystemTime::now(),
            ctime: SystemTime::now(),
            crtime: SystemTime::now(),
            kind: FileType::RegularFile,
            perm: 0o444, // read-only
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
            blksize: 512,
            flags: 0,
        }
    }

    fn dir_attr(&self, ino: u64) -> FileAttr {
        FileAttr {
            ino,
            size: 0,
            blocks: 0,
            atime: SystemTime::now(),
            mtime: SystemTime::now(),
            ctime: SystemTime::now(),
            crtime: SystemTime::now(),
            kind: FileType::Directory,
            perm: 0o755,
            nlink: 2,
            uid: 0,
            gid: 0,
            rdev: 0,
            blksize: 512,
            flags: 0,
        }
    }

    fn fetch_file(&self, rel_path: &str) -> Result<Vec<u8>, i32> {
        // Check cache first.
        {
            let cache = self.file_cache.read().unwrap();
            if let Some(data) = cache.get(rel_path) {
                return Ok(data.clone());
            }
        }

        // Fetch from gateway.
        let params =
            rmpv::ext::to_value(&serde_json::json!({"path": rel_path})).map_err(|_| libc::EIO)?;
        let resp = self
            .channel
            .request_blocking("skills/read", params)
            .map_err(|e| {
                warn!("skills/read {rel_path} failed: {e}");
                libc::EIO
            })?;

        let parsed: SkillsReadResponse = rmpv::ext::from_value(resp).map_err(|e| {
            warn!("parse skills/read {rel_path} response failed: {e}");
            libc::EIO
        })?;
        let data = parsed.data;

        // Cache the result.
        self.file_cache
            .write()
            .unwrap()
            .insert(rel_path.to_string(), data.clone());

        Ok(data)
    }
}

impl Filesystem for SkillsFuseFs {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = name.to_string_lossy();
        let tree = self.tree.read().unwrap();

        let Some(FsNode::Dir { children }) = tree.nodes.get(&parent) else {
            reply.error(libc::ENOENT);
            return;
        };

        // Find child by basename within this directory.
        for &child_ino in children {
            if let Some(node) = tree.nodes.get(&child_ino) {
                let child_name = match node {
                    FsNode::Dir { .. } => tree
                        .path_to_inode
                        .iter()
                        .find(|(_, ino)| **ino == child_ino)
                        .map(|(path, _)| path.split('/').next_back().unwrap_or(path)),
                    FsNode::File { rel_path, .. } => {
                        Some(rel_path.split('/').next_back().unwrap_or(rel_path))
                    }
                };
                if child_name == Some(name_str.as_ref()) {
                    let attr = match node {
                        FsNode::Dir { .. } => self.dir_attr(child_ino),
                        FsNode::File { size, .. } => self.file_attr(child_ino, *size),
                    };
                    reply.entry(&TTL, &attr, 0);
                    return;
                }
            }
        }

        reply.error(libc::ENOENT);
    }

    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        let tree = self.tree.read().unwrap();

        if ino == 1 {
            reply.attr(&TTL, &tree.root_attr());
            return;
        }

        match tree.nodes.get(&ino) {
            Some(FsNode::Dir { .. }) => {
                reply.attr(&TTL, &self.dir_attr(ino));
            }
            Some(FsNode::File { rel_path, size }) => {
                // If size is 0, try fetching to get real size.
                let actual_size = if *size == 0 {
                    match self.fetch_file(rel_path) {
                        Ok(data) => data.len() as u64,
                        Err(_) => 0,
                    }
                } else {
                    *size
                };
                reply.attr(&TTL, &self.file_attr(ino, actual_size));
            }
            None => {
                reply.error(libc::ENOENT);
            }
        }
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData,
    ) {
        let tree = self.tree.read().unwrap();
        let Some(FsNode::File { rel_path, .. }) = tree.nodes.get(&ino) else {
            reply.error(libc::EINVAL);
            return;
        };
        let rel_path = rel_path.clone();
        drop(tree);

        let data = match self.fetch_file(&rel_path) {
            Ok(d) => d,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        let start = offset.max(0) as usize;
        if start >= data.len() {
            reply.data(&[]);
            return;
        }
        let end = (start + size as usize).min(data.len());
        reply.data(&data[start..end]);
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let tree = self.tree.read().unwrap();

        let Some(FsNode::Dir { children }) = tree.nodes.get(&ino) else {
            reply.error(libc::ENOTDIR);
            return;
        };

        let entries: Vec<(u64, FileType, String)> =
            std::iter::once((ino, FileType::Directory, ".".into()))
                .chain(std::iter::once((ino, FileType::Directory, "..".into())))
                .chain(children.iter().filter_map(|&child_ino| {
                    let node = tree.nodes.get(&child_ino)?;
                    let name = match node {
                        FsNode::Dir { .. } => tree
                            .path_to_inode
                            .iter()
                            .find(|(_, ino)| **ino == child_ino)
                            .map(|(path, _)| {
                                path.split('/').next_back().unwrap_or(path).to_string()
                            })
                            .unwrap_or_else(|| format!("dir-{child_ino}")),
                        FsNode::File { rel_path, .. } => {
                            rel_path.split('/').last().unwrap_or(rel_path).to_string()
                        }
                    };
                    let kind = match node {
                        FsNode::Dir { .. } => FileType::Directory,
                        FsNode::File { .. } => FileType::RegularFile,
                    };
                    Some((child_ino, kind, name))
                }))
                .collect();

        for (i, (ino, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
            if reply.add(*ino, (i + 1) as i64, *kind, name) {
                break;
            }
        }
        reply.ok();
    }
}

/// Skills info from gateway (matches gateway SkillsInfo struct).
#[derive(serde::Deserialize)]
struct SkillsListResponse {
    skills: Vec<SkillInfo>,
}

#[derive(serde::Deserialize)]
struct SkillsReadResponse {
    #[serde(with = "serde_bytes")]
    data: Vec<u8>,
}

#[derive(serde::Deserialize)]
struct SkillInfo {
    name: String,
    #[serde(default)]
    _description: String,
    files: Vec<SkillFileInfo>,
}

#[derive(serde::Deserialize)]
struct SkillFileInfo {
    path: String,
    #[serde(default)]
    size: u64,
}
