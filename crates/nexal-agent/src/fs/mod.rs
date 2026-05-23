//! Filesystem component — trait definition and implementations.

pub mod local;
pub mod skills;

use std::io;

use async_trait::async_trait;
use nexal_utils_absolute_path::AbsolutePathBuf;

// ── Trait ──────────────────────────────────────────────────────────

pub type FileSystemResult<T> = io::Result<T>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreateDirectoryOptions {
    pub recursive: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoveOptions {
    pub recursive: bool,
    pub force: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopyOptions {
    pub recursive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadata {
    pub is_directory: bool,
    pub is_file: bool,
    pub created_at_ms: i64,
    pub modified_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadDirectoryEntry {
    pub file_name: String,
    pub is_directory: bool,
    pub is_file: bool,
}

#[async_trait]
pub trait ExecutorFileSystem: Send + Sync {
    async fn read_file(&self, path: &AbsolutePathBuf) -> FileSystemResult<Vec<u8>>;

    async fn write_file(&self, path: &AbsolutePathBuf, contents: Vec<u8>) -> FileSystemResult<()>;

    async fn create_directory(
        &self,
        path: &AbsolutePathBuf,
        options: CreateDirectoryOptions,
    ) -> FileSystemResult<()>;

    async fn get_metadata(&self, path: &AbsolutePathBuf) -> FileSystemResult<FileMetadata>;

    async fn read_directory(
        &self,
        path: &AbsolutePathBuf,
    ) -> FileSystemResult<Vec<ReadDirectoryEntry>>;

    async fn remove(&self, path: &AbsolutePathBuf, options: RemoveOptions) -> FileSystemResult<()>;

    async fn copy(
        &self,
        source_path: &AbsolutePathBuf,
        destination_path: &AbsolutePathBuf,
        options: CopyOptions,
    ) -> FileSystemResult<()>;
}
