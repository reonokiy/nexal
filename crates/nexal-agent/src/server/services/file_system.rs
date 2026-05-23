use std::io;

use crate::fs::local::LocalFileSystem;
use crate::fs::{CopyOptions, CreateDirectoryOptions, ExecutorFileSystem, RemoveOptions};
use crate::protocol::channel::{internal_error, invalid_request};
use crate::protocol::errors::ChannelError;
use crate::protocol::wire::{
    FsCopyParams, FsCopyResponse, FsCreateDirectoryParams, FsCreateDirectoryResponse,
    FsGetMetadataParams, FsGetMetadataResponse, FsReadDirectoryEntry, FsReadDirectoryParams,
    FsReadDirectoryResponse, FsReadFileParams, FsReadFileResponse, FsRemoveParams,
    FsRemoveResponse, FsWriteFileParams, FsWriteFileResponse,
};

#[derive(Clone, Default)]
pub(crate) struct FileSystemHandler {
    file_system: LocalFileSystem,
}

impl FileSystemHandler {
    pub(crate) async fn read_file(
        &self,
        params: FsReadFileParams,
    ) -> Result<FsReadFileResponse, ChannelError> {
        let bytes = self
            .file_system
            .read_file(&params.path)
            .await
            .map_err(map_fs_error)?;
        Ok(FsReadFileResponse { data: bytes })
    }

    pub(crate) async fn write_file(
        &self,
        params: FsWriteFileParams,
    ) -> Result<FsWriteFileResponse, ChannelError> {
        self.file_system
            .write_file(&params.path, params.data)
            .await
            .map_err(map_fs_error)?;
        Ok(FsWriteFileResponse {})
    }

    pub(crate) async fn create_directory(
        &self,
        params: FsCreateDirectoryParams,
    ) -> Result<FsCreateDirectoryResponse, ChannelError> {
        self.file_system
            .create_directory(
                &params.path,
                CreateDirectoryOptions {
                    recursive: params.recursive.unwrap_or(true),
                },
            )
            .await
            .map_err(map_fs_error)?;
        Ok(FsCreateDirectoryResponse {})
    }

    pub(crate) async fn get_metadata(
        &self,
        params: FsGetMetadataParams,
    ) -> Result<FsGetMetadataResponse, ChannelError> {
        let metadata = self
            .file_system
            .get_metadata(&params.path)
            .await
            .map_err(map_fs_error)?;
        Ok(FsGetMetadataResponse {
            is_directory: metadata.is_directory,
            is_file: metadata.is_file,
            created_at_ms: metadata.created_at_ms,
            modified_at_ms: metadata.modified_at_ms,
        })
    }

    pub(crate) async fn read_directory(
        &self,
        params: FsReadDirectoryParams,
    ) -> Result<FsReadDirectoryResponse, ChannelError> {
        let entries = self
            .file_system
            .read_directory(&params.path)
            .await
            .map_err(map_fs_error)?;
        Ok(FsReadDirectoryResponse {
            entries: entries
                .into_iter()
                .map(|entry| FsReadDirectoryEntry {
                    file_name: entry.file_name,
                    is_directory: entry.is_directory,
                    is_file: entry.is_file,
                })
                .collect(),
        })
    }

    pub(crate) async fn remove(
        &self,
        params: FsRemoveParams,
    ) -> Result<FsRemoveResponse, ChannelError> {
        self.file_system
            .remove(
                &params.path,
                RemoveOptions {
                    recursive: params.recursive.unwrap_or(true),
                    force: params.force.unwrap_or(true),
                },
            )
            .await
            .map_err(map_fs_error)?;
        Ok(FsRemoveResponse {})
    }

    pub(crate) async fn copy(&self, params: FsCopyParams) -> Result<FsCopyResponse, ChannelError> {
        self.file_system
            .copy(
                &params.source_path,
                &params.destination_path,
                CopyOptions {
                    recursive: params.recursive,
                },
            )
            .await
            .map_err(map_fs_error)?;
        Ok(FsCopyResponse {})
    }
}

fn map_fs_error(err: io::Error) -> ChannelError {
    if err.kind() == io::ErrorKind::InvalidInput {
        invalid_request(err.to_string())
    } else {
        internal_error(err.to_string())
    }
}
