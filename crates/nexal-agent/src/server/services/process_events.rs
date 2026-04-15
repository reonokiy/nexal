use std::sync::Arc;

use tokio::sync::broadcast;

use crate::protocol::{
    ExecClosedNotification, ExecExitedNotification, ExecOutputDeltaNotification,
};

#[derive(Debug, Clone)]
pub(crate) enum ProcessEvent {
    OutputDelta(ExecOutputDeltaNotification),
    Exited(ExecExitedNotification),
    Closed(ExecClosedNotification),
}

#[derive(Clone)]
pub(crate) struct ProcessEventBroadcaster {
    tx: Arc<broadcast::Sender<ProcessEvent>>,
}

impl Default for ProcessEventBroadcaster {
    fn default() -> Self {
        let (tx, _rx) = broadcast::channel(256);
        Self { tx: Arc::new(tx) }
    }
}

impl ProcessEventBroadcaster {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<ProcessEvent> {
        self.tx.subscribe()
    }

    pub(crate) fn send_output_delta(&self, notification: ExecOutputDeltaNotification) {
        let _ = self.tx.send(ProcessEvent::OutputDelta(notification));
    }

    pub(crate) fn send_exited(&self, notification: ExecExitedNotification) {
        let _ = self.tx.send(ProcessEvent::Exited(notification));
    }

    pub(crate) fn send_closed(&self, notification: ExecClosedNotification) {
        let _ = self.tx.send(ProcessEvent::Closed(notification));
    }
}
