#![cfg(unix)]

use std::sync::Arc;

use anyhow::Result;
use nexal_agent::Environment;
use nexal_agent::ExecBackend;
use nexal_agent::ExecParams;
use nexal_agent::ExecProcess;
use nexal_agent::ProcessId;
use nexal_agent::ReadResponse;
use nexal_agent::StartedExecProcess;
use pretty_assertions::assert_eq;
use tokio::sync::watch;
use tokio::time::Duration;
use tokio::time::timeout;

async fn create_backend() -> Result<Arc<dyn ExecBackend>> {
    Ok(Environment::create().await?.get_exec_backend())
}

async fn read_process_until_change(
    session: Arc<dyn ExecProcess>,
    wake_rx: &mut watch::Receiver<u64>,
    after_seq: Option<u64>,
) -> Result<ReadResponse> {
    let response = session
        .read(after_seq, /*max_bytes*/ None, /*wait_ms*/ Some(0))
        .await?;
    if !response.chunks.is_empty() || response.closed || response.failure.is_some() {
        return Ok(response);
    }

    timeout(Duration::from_secs(2), wake_rx.changed()).await??;
    session
        .read(after_seq, /*max_bytes*/ None, /*wait_ms*/ Some(0))
        .await
        .map_err(Into::into)
}

async fn collect_process_output_from_reads(
    session: Arc<dyn ExecProcess>,
    mut wake_rx: watch::Receiver<u64>,
) -> Result<(String, Option<i32>, bool)> {
    let mut output = String::new();
    let mut exit_code = None;
    let mut after_seq = None;
    loop {
        let response =
            read_process_until_change(Arc::clone(&session), &mut wake_rx, after_seq).await?;
        if let Some(message) = response.failure {
            anyhow::bail!("process failed before closed state: {message}");
        }
        for chunk in response.chunks {
            output.push_str(&String::from_utf8_lossy(&chunk.chunk.into_inner()));
            after_seq = Some(chunk.seq);
        }
        if response.exited {
            exit_code = response.exit_code;
        }
        if response.closed {
            break;
        }
        after_seq = response.next_seq.checked_sub(1).or(after_seq);
    }
    drop(session);
    Ok((output, exit_code, true))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_process_starts_and_exits() -> Result<()> {
    let backend = create_backend().await?;
    let session = backend
        .start(ExecParams {
            process_id: ProcessId::from("proc-1"),
            argv: vec!["true".to_string()],
            cwd: std::env::current_dir()?,
            env: Default::default(),
            tty: false,
            arg0: None,
        })
        .await?;
    assert_eq!(session.process.process_id().as_str(), "proc-1");
    let wake_rx = session.process.subscribe_wake();
    let (_, exit_code, closed) =
        collect_process_output_from_reads(session.process, wake_rx).await?;

    assert_eq!(exit_code, Some(0));
    assert!(closed);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_process_streams_output() -> Result<()> {
    let backend = create_backend().await?;
    let process_id = "proc-stream".to_string();
    let session = backend
        .start(ExecParams {
            process_id: process_id.clone().into(),
            argv: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "sleep 0.05; printf 'session output\\n'".to_string(),
            ],
            cwd: std::env::current_dir()?,
            env: Default::default(),
            tty: false,
            arg0: None,
        })
        .await?;
    assert_eq!(session.process.process_id().as_str(), process_id);

    let StartedExecProcess { process } = session;
    let wake_rx = process.subscribe_wake();
    let (output, exit_code, closed) = collect_process_output_from_reads(process, wake_rx).await?;
    assert_eq!(output, "session output\n");
    assert_eq!(exit_code, Some(0));
    assert!(closed);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_process_write_then_read() -> Result<()> {
    let backend = create_backend().await?;
    let process_id = "proc-stdin".to_string();
    let session = backend
        .start(ExecParams {
            process_id: process_id.clone().into(),
            argv: vec![
                "/usr/bin/python3".to_string(),
                "-c".to_string(),
                "import sys; line = sys.stdin.readline(); sys.stdout.write(f'from-stdin:{line}'); sys.stdout.flush()".to_string(),
            ],
            cwd: std::env::current_dir()?,
            env: Default::default(),
            tty: true,
            arg0: None,
        })
        .await?;
    assert_eq!(session.process.process_id().as_str(), process_id);

    tokio::time::sleep(Duration::from_millis(200)).await;
    session.process.write(b"hello\n".to_vec()).await?;
    let StartedExecProcess { process } = session;
    let wake_rx = process.subscribe_wake();
    let (output, exit_code, closed) = collect_process_output_from_reads(process, wake_rx).await?;

    assert!(
        output.contains("from-stdin:hello"),
        "unexpected output: {output:?}"
    );
    assert_eq!(exit_code, Some(0));
    assert!(closed);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_process_preserves_queued_events_before_subscribe() -> Result<()> {
    let backend = create_backend().await?;
    let session = backend
        .start(ExecParams {
            process_id: ProcessId::from("proc-queued"),
            argv: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "printf 'queued output\\n'".to_string(),
            ],
            cwd: std::env::current_dir()?,
            env: Default::default(),
            tty: false,
            arg0: None,
        })
        .await?;

    tokio::time::sleep(Duration::from_millis(200)).await;

    let StartedExecProcess { process } = session;
    let wake_rx = process.subscribe_wake();
    let (output, exit_code, closed) = collect_process_output_from_reads(process, wake_rx).await?;
    assert_eq!(output, "queued output\n");
    assert_eq!(exit_code, Some(0));
    assert!(closed);
    Ok(())
}
