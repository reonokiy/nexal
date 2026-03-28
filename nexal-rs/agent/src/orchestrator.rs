//! Multi-agent orchestrator — decomposes tasks and delegates to worker agents.
//!
//! The orchestrator manages a pool of worker `AgentHandle`s, each running
//! independently. Sub-tasks can have dependencies (must wait for others
//! to complete before starting).

use std::collections::{HashMap, VecDeque};


use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::actor::{AgentEvent, AgentHandle, AgentMessage};

/// Unique task identifier.
pub type TaskId = u32;

/// A sub-task to be executed by a worker agent.
#[derive(Debug, Clone)]
pub struct SubTask {
    pub id: TaskId,
    pub prompt: String,
    /// Dependencies: must complete before this starts.
    pub depends_on: Vec<TaskId>,
}

/// Result of a completed sub-task.
#[derive(Debug, Clone)]
pub struct TaskResult {
    pub id: TaskId,
    pub output: String,
}

/// Orchestrator that manages parallel agent workers.
pub struct Orchestrator {
    workers: Vec<WorkerEntry>,
    task_queue: VecDeque<SubTask>,
    results: HashMap<TaskId, TaskResult>,
    completed: Vec<TaskId>,
    next_task_id: TaskId,
    event_tx: mpsc::Sender<OrchestratorEvent>,
}

struct WorkerEntry {
    handle: AgentHandle,
    busy: bool,
    current_task: Option<TaskId>,
}

/// Events emitted by the orchestrator.
#[derive(Debug, Clone)]
pub enum OrchestratorEvent {
    /// A sub-task was assigned to a worker.
    TaskStarted { task_id: TaskId, worker_idx: usize },
    /// A sub-task completed.
    TaskCompleted { task_id: TaskId, output: String },
    /// All tasks done.
    AllDone { results: Vec<TaskResult> },
    /// Error from a worker.
    Error { task_id: TaskId, message: String },
}

impl Orchestrator {
    pub fn new(event_tx: mpsc::Sender<OrchestratorEvent>) -> Self {
        Self {
            workers: Vec::new(),
            task_queue: VecDeque::new(),
            results: HashMap::new(),
            completed: Vec::new(),
            next_task_id: 1,
            event_tx,
        }
    }

    /// Add a worker agent.
    pub fn add_worker(&mut self, handle: AgentHandle) {
        self.workers.push(WorkerEntry {
            handle,
            busy: false,
            current_task: None,
        });
    }

    /// Create a sub-task and return its ID.
    pub fn create_task(&mut self, prompt: String, depends_on: Vec<TaskId>) -> TaskId {
        let id = self.next_task_id;
        self.next_task_id += 1;
        self.task_queue.push_back(SubTask {
            id,
            prompt,
            depends_on,
        });
        id
    }

    /// Run all queued tasks. Blocks until all are complete.
    pub async fn run(&mut self, agent_event_rx: &mut mpsc::Receiver<AgentEvent>) {
        if self.workers.is_empty() {
            warn!("no workers available");
            return;
        }

        info!(
            tasks = self.task_queue.len(),
            workers = self.workers.len(),
            "orchestrator starting"
        );

        loop {
            // Try to dispatch ready tasks to idle workers
            self.dispatch_ready_tasks().await;

            // Check if all done
            if self.task_queue.is_empty() && self.workers.iter().all(|w| !w.busy) {
                let results: Vec<TaskResult> = self
                    .completed
                    .iter()
                    .filter_map(|id| self.results.get(id).cloned())
                    .collect();
                let _ = self.event_tx.send(OrchestratorEvent::AllDone { results }).await;
                break;
            }

            // Wait for a worker to complete
            if let Some(event) = agent_event_rx.recv().await {
                self.handle_agent_event(event).await;
            } else {
                break;
            }
        }
    }

    async fn dispatch_ready_tasks(&mut self) {
        let mut dispatched = Vec::new();

        for (queue_idx, task) in self.task_queue.iter().enumerate() {
            // Check dependencies
            let deps_met = task
                .depends_on
                .iter()
                .all(|dep| self.results.contains_key(dep));
            if !deps_met {
                continue;
            }

            // Find an idle worker
            if let Some(worker_idx) = self.workers.iter().position(|w| !w.busy) {
                dispatched.push((queue_idx, worker_idx, task.id, task.prompt.clone()));
            }
        }

        // Remove from queue (reverse order to keep indices valid)
        for (queue_idx, worker_idx, task_id, prompt) in dispatched.into_iter().rev() {
            self.task_queue.remove(queue_idx);

            let worker = &mut self.workers[worker_idx];
            worker.busy = true;
            worker.current_task = Some(task_id);

            debug!(task = task_id, worker = worker_idx, "dispatching task");
            let _ = self.event_tx.send(OrchestratorEvent::TaskStarted {
                task_id,
                worker_idx,
            }).await;

            let _ = worker
                .handle
                .send(AgentMessage::UserInput {
                    text: prompt,
                    sender: "orchestrator".to_string(),
                    channel: "internal".to_string(),
                })
                .await;
        }
    }

    async fn handle_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::Response {
                session_key: _,
                chunks,
                ..
            } => {
                let output = chunks.join("\n\n");

                // Find which worker/task this belongs to
                if let Some(worker) = self
                    .workers
                    .iter_mut()
                    .find(|w| w.busy)
                {
                    if let Some(task_id) = worker.current_task.take() {
                        worker.busy = false;
                        self.results.insert(
                            task_id,
                            TaskResult {
                                id: task_id,
                                output: output.clone(),
                            },
                        );
                        self.completed.push(task_id);
                        let _ = self.event_tx.send(OrchestratorEvent::TaskCompleted {
                            task_id,
                            output,
                        }).await;
                    }
                }
            }
            AgentEvent::Error {
                session_key: _,
                message,
            } => {
                if let Some(worker) = self.workers.iter_mut().find(|w| w.busy) {
                    if let Some(task_id) = worker.current_task.take() {
                        worker.busy = false;
                        let _ = self.event_tx.send(OrchestratorEvent::Error {
                            task_id,
                            message,
                        }).await;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_creation() {
        let (tx, _rx) = mpsc::channel(16);
        let mut orch = Orchestrator::new(tx);

        let t1 = orch.create_task("do thing 1".into(), vec![]);
        let t2 = orch.create_task("do thing 2".into(), vec![t1]);
        let t3 = orch.create_task("merge results".into(), vec![t1, t2]);

        assert_eq!(t1, 1);
        assert_eq!(t2, 2);
        assert_eq!(t3, 3);
        assert_eq!(orch.task_queue.len(), 3);
        assert_eq!(orch.task_queue[1].depends_on, vec![1]);
        assert_eq!(orch.task_queue[2].depends_on, vec![1, 2]);
    }
}
