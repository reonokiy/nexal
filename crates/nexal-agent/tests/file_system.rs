#![cfg(unix)]

use std::os::unix::fs::symlink;
use std::process::Command;
use std::sync::Arc;

use anyhow::Result;
use nexal_agent::{
    CopyOptions, CreateDirectoryOptions, Environment, ExecutorFileSystem, ReadDirectoryEntry,
    RemoveOptions,
};
use nexal_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

fn abs(path: std::path::PathBuf) -> AbsolutePathBuf {
    assert!(
        path.is_absolute(),
        "path must be absolute: {}",
        path.display()
    );
    AbsolutePathBuf::try_from(path).expect("path should be absolute")
}

async fn create_fs() -> Result<Arc<dyn ExecutorFileSystem>> {
    Ok(Environment::create().await?.get_filesystem())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_metadata_returns_expected_fields() -> Result<()> {
    let fs = create_fs().await?;

    let tmp = TempDir::new()?;
    let file_path = tmp.path().join("note.txt");
    std::fs::write(&file_path, b"hello")?;

    let metadata = fs.get_metadata(&abs(file_path)).await?;
    assert!(!metadata.is_directory);
    assert!(metadata.is_file);
    assert!(metadata.modified_at_ms > 0);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn methods_cover_surface_area() -> Result<()> {
    let fs = create_fs().await?;

    let tmp = TempDir::new()?;
    let source_dir = tmp.path().join("source");
    let nested_dir = source_dir.join("nested");
    let source_file = source_dir.join("root.txt");
    let nested_file = nested_dir.join("note.txt");
    let copied_dir = tmp.path().join("copied");
    let copied_file = tmp.path().join("copy.txt");

    fs.create_directory(
        &abs(nested_dir.clone()),
        CreateDirectoryOptions { recursive: true },
    )
    .await?;

    fs.write_file(&abs(nested_file.clone()), b"hello from trait".to_vec())
        .await?;
    fs.write_file(
        &abs(source_file.clone()),
        b"hello from source root".to_vec(),
    )
    .await?;

    assert_eq!(
        fs.read_file(&abs(nested_file.clone())).await?,
        b"hello from trait"
    );

    fs.copy(
        &abs(nested_file),
        &abs(copied_file.clone()),
        CopyOptions { recursive: false },
    )
    .await?;
    assert_eq!(std::fs::read_to_string(&copied_file)?, "hello from trait");

    fs.copy(
        &abs(source_dir.clone()),
        &abs(copied_dir.clone()),
        CopyOptions { recursive: true },
    )
    .await?;
    assert_eq!(
        std::fs::read_to_string(copied_dir.join("nested").join("note.txt"))?,
        "hello from trait"
    );

    let mut entries = fs.read_directory(&abs(source_dir)).await?;
    entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    assert_eq!(
        entries,
        vec![
            ReadDirectoryEntry {
                file_name: "nested".into(),
                is_directory: true,
                is_file: false
            },
            ReadDirectoryEntry {
                file_name: "root.txt".into(),
                is_directory: false,
                is_file: true
            },
        ]
    );

    fs.remove(
        &abs(copied_dir.clone()),
        RemoveOptions {
            recursive: true,
            force: true,
        },
    )
    .await?;
    assert!(!copied_dir.exists());

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_rejects_directory_without_recursive() -> Result<()> {
    let fs = create_fs().await?;

    let tmp = TempDir::new()?;
    let source_dir = tmp.path().join("source");
    std::fs::create_dir_all(&source_dir)?;

    let error = fs
        .copy(
            &abs(source_dir),
            &abs(tmp.path().join("dest")),
            CopyOptions { recursive: false },
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(
        error.to_string(),
        "fs/copy requires recursive: true when sourcePath is a directory"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_rejects_copying_directory_into_descendant() -> Result<()> {
    let fs = create_fs().await?;

    let tmp = TempDir::new()?;
    let source_dir = tmp.path().join("source");
    std::fs::create_dir_all(source_dir.join("nested"))?;

    let error = fs
        .copy(
            &abs(source_dir.clone()),
            &abs(source_dir.join("nested").join("copy")),
            CopyOptions { recursive: true },
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(
        error.to_string(),
        "fs/copy cannot copy a directory to itself or one of its descendants"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_preserves_symlinks_in_recursive_copy() -> Result<()> {
    let fs = create_fs().await?;

    let tmp = TempDir::new()?;
    let source_dir = tmp.path().join("source");
    let nested_dir = source_dir.join("nested");
    let copied_dir = tmp.path().join("copied");
    std::fs::create_dir_all(&nested_dir)?;
    symlink("nested", source_dir.join("nested-link"))?;

    fs.copy(
        &abs(source_dir),
        &abs(copied_dir.clone()),
        CopyOptions { recursive: true },
    )
    .await?;

    let copied_link = copied_dir.join("nested-link");
    let metadata = std::fs::symlink_metadata(&copied_link)?;
    assert!(metadata.file_type().is_symlink());
    assert_eq!(
        std::fs::read_link(copied_link)?,
        std::path::PathBuf::from("nested")
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_ignores_unknown_special_files_in_recursive_copy() -> Result<()> {
    let fs = create_fs().await?;

    let tmp = TempDir::new()?;
    let source_dir = tmp.path().join("source");
    let copied_dir = tmp.path().join("copied");
    std::fs::create_dir_all(&source_dir)?;
    std::fs::write(source_dir.join("note.txt"), "hello")?;

    let fifo_path = source_dir.join("named-pipe");
    let output = Command::new("mkfifo").arg(&fifo_path).output()?;
    anyhow::ensure!(
        output.status.success(),
        "mkfifo failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    );

    fs.copy(
        &abs(source_dir),
        &abs(copied_dir.clone()),
        CopyOptions { recursive: true },
    )
    .await?;

    assert_eq!(
        std::fs::read_to_string(copied_dir.join("note.txt"))?,
        "hello"
    );
    assert!(!copied_dir.join("named-pipe").exists());

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_rejects_standalone_fifo_source() -> Result<()> {
    let fs = create_fs().await?;

    let tmp = TempDir::new()?;
    let fifo_path = tmp.path().join("named-pipe");
    let output = Command::new("mkfifo").arg(&fifo_path).output()?;
    anyhow::ensure!(
        output.status.success(),
        "mkfifo failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    );

    let error = fs
        .copy(
            &abs(fifo_path),
            &abs(tmp.path().join("copied")),
            CopyOptions { recursive: false },
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(
        error.to_string(),
        "fs/copy only supports regular files, directories, and symlinks"
    );

    Ok(())
}
