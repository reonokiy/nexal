use std::sync::Arc;

use nexal_core::NexalThread;
use nexal_core::NewThread;
use nexal_core::ThreadManager;
use nexal_core::config::Config;
use nexal_protocol::protocol::Event;
use nexal_protocol::protocol::EventMsg;
use nexal_protocol::protocol::Op;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::mpsc::unbounded_channel;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;

const TUI_NOTIFY_CLIENT: &str = "nexal-tui";

async fn initialize_app_server_client_name(thread: &NexalThread) {
    if let Err(err) = thread
        .set_app_server_client_name(Some(TUI_NOTIFY_CLIENT.to_string()))
        .await
    {
        tracing::error!("failed to set app server client name: {err}");
    }
}

/// Spawn the agent bootstrapper and op forwarding loop, returning the
/// `UnboundedSender<Op>` used by the UI to submit operations.
pub(crate) fn spawn_agent(
    config: Config,
    app_event_tx: AppEventSender,
    server: Arc<ThreadManager>,
) -> UnboundedSender<Op> {
    let (nexal_op_tx, mut nexal_op_rx) = unbounded_channel::<Op>();

    let app_event_tx_clone = app_event_tx;
    tokio::spawn(async move {
        let NewThread {
            thread,
            session_configured,
            ..
        } = match server.start_thread(config).await {
            Ok(v) => v,
            Err(err) => {
                let message = format!("Failed to initialize nexal: {err}");
                tracing::error!("{message}");
                app_event_tx_clone.send(AppEvent::NexalEvent(Event {
                    id: "".to_string(),
                    msg: EventMsg::Error(err.to_error_event(/*message_prefix*/ None)),
                }));
                app_event_tx_clone.send(AppEvent::FatalExitRequest(message));
                tracing::error!("failed to initialize nexal: {err}");
                return;
            }
        };
        initialize_app_server_client_name(thread.as_ref()).await;

        // Forward the captured `SessionConfigured` event so it can be rendered in the UI.
        let ev = nexal_protocol::protocol::Event {
            // The `id` does not matter for rendering, so we can use a fake value.
            id: "".to_string(),
            msg: nexal_protocol::protocol::EventMsg::SessionConfigured(session_configured),
        };
        app_event_tx_clone.send(AppEvent::NexalEvent(ev));

        let thread_clone = thread.clone();
        tokio::spawn(async move {
            while let Some(op) = nexal_op_rx.recv().await {
                let id = thread_clone.submit(op).await;
                if let Err(e) = id {
                    tracing::error!("failed to submit op: {e}");
                }
            }
        });

        while let Ok(event) = thread.next_event().await {
            let is_shutdown_complete = matches!(event.msg, EventMsg::ShutdownComplete);
            app_event_tx_clone.send(AppEvent::NexalEvent(event));
            if is_shutdown_complete {
                // ShutdownComplete is terminal for a thread; drop this receiver task so
                // the Arc<NexalThread> can be released and thread resources can clean up.
                break;
            }
        }
    });

    nexal_op_tx
}

/// Spawn agent loops for an existing thread (e.g., a forked thread).
/// Sends the provided `SessionConfiguredEvent` immediately, then forwards subsequent
/// events and accepts Ops for submission.
pub(crate) fn spawn_agent_from_existing(
    thread: std::sync::Arc<NexalThread>,
    session_configured: nexal_protocol::protocol::SessionConfiguredEvent,
    app_event_tx: AppEventSender,
) -> UnboundedSender<Op> {
    let (nexal_op_tx, mut nexal_op_rx) = unbounded_channel::<Op>();

    let app_event_tx_clone = app_event_tx;
    tokio::spawn(async move {
        initialize_app_server_client_name(thread.as_ref()).await;

        // Forward the captured `SessionConfigured` event so it can be rendered in the UI.
        let ev = nexal_protocol::protocol::Event {
            id: "".to_string(),
            msg: nexal_protocol::protocol::EventMsg::SessionConfigured(session_configured),
        };
        app_event_tx_clone.send(AppEvent::NexalEvent(ev));

        let thread_clone = thread.clone();
        tokio::spawn(async move {
            while let Some(op) = nexal_op_rx.recv().await {
                let id = thread_clone.submit(op).await;
                if let Err(e) = id {
                    tracing::error!("failed to submit op: {e}");
                }
            }
        });

        while let Ok(event) = thread.next_event().await {
            let is_shutdown_complete = matches!(event.msg, EventMsg::ShutdownComplete);
            app_event_tx_clone.send(AppEvent::NexalEvent(event));
            if is_shutdown_complete {
                // ShutdownComplete is terminal for a thread; drop this receiver task so
                // the Arc<NexalThread> can be released and thread resources can clean up.
                break;
            }
        }
    });

    nexal_op_tx
}

/// Spawn an op-forwarding loop for an existing thread without subscribing to events.
pub(crate) fn spawn_op_forwarder(thread: std::sync::Arc<NexalThread>) -> UnboundedSender<Op> {
    let (nexal_op_tx, mut nexal_op_rx) = unbounded_channel::<Op>();

    tokio::spawn(async move {
        initialize_app_server_client_name(thread.as_ref()).await;
        while let Some(op) = nexal_op_rx.recv().await {
            if let Err(e) = thread.submit(op).await {
                tracing::error!("failed to submit op: {e}");
            }
        }
    });

    nexal_op_tx
}
