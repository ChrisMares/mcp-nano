use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use tokio_util::sync::CancellationToken;

use super::progress::ProgressCallback;

/// A boxed future returned by task functions. `'static` so it owns all
/// captured state (the closure can clone an `Arc<IngestionService>` into it).
pub type BoxedTaskFuture =
    Pin<Box<dyn Future<Output = Result<String>> + Send + 'static>>;

/// A task function: given the deserialized `task_params` JSON, a progress
/// callback and the job's cancellation token, run the task to completion.
///
/// The task reports an early stop by returning [`super::JobCancelled`]; the
/// worker then marks the job `CANCELLED` and deletes anything it wrote.
///
/// Held inside an `Arc` so the registry can call it repeatedly without
/// moving the captured services (e.g. `Arc<IngestionService>`).
pub type TaskFn = Arc<
    dyn Fn(serde_json::Value, ProgressCallback, CancellationToken) -> BoxedTaskFuture
        + Send
        + Sync,
>;

/// Maps `job_status.task_name` -> callable. Built once at startup and
/// injected into the worker.
#[derive(Clone, Default)]
pub struct TaskRegistry {
    tasks: HashMap<String, TaskFn>,
}

impl TaskRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<F, Fut>(&mut self, name: impl Into<String>, task: F)
    where
        F: Fn(serde_json::Value, ProgressCallback, CancellationToken) -> Fut
            + Send
            + Sync
            + 'static,
        Fut: Future<Output = Result<String>> + Send + 'static,
    {
        let name = name.into();
        let task: TaskFn =
            Arc::new(move |params, progress, cancel| Box::pin(task(params, progress, cancel)));
        self.tasks.insert(name, task);
    }

    pub fn get(&self, name: &str) -> Option<&TaskFn> {
        self.tasks.get(name)
    }

    pub fn names(&self) -> Vec<String> {
        self.tasks.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::progress::noop_progress;

    #[tokio::test]
    async fn registry_dispatches_by_name() {
        let mut reg = TaskRegistry::new();
        reg.register("echo", |params, _progress, _cancel| async move {
            let s = params
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_string();
            Ok(s)
        });
        let task = reg.get("echo").expect("registered");
        let result = task(
            serde_json::json!({"msg": "hi"}),
            noop_progress(),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(result.unwrap(), "hi");
    }

    #[tokio::test]
    async fn registry_returns_none_for_unknown_task() {
        let reg = TaskRegistry::new();
        assert!(reg.get("nope").is_none());
    }

    #[test]
    fn registry_names_lists_registered_tasks() {
        let mut reg = TaskRegistry::new();
        reg.register("a", |_, _, _| async { Ok("a".into()) });
        reg.register("b", |_, _, _| async { Ok("b".into()) });
        let mut names = reg.names();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn progress_callback_is_awaitable() {
        // Verify the async callback signature compiles and runs.
        let mut reg = TaskRegistry::new();
        reg.register("with_progress", |_params, progress, _cancel| async move {
            progress(10, Some("ten".to_string())).await;
            progress(100, Some("done".to_string())).await;
            Ok("ok".to_string())
        });
        let task = reg.get("with_progress").expect("registered");
        let result = task(
            serde_json::json!({}),
            noop_progress(),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(result.unwrap(), "ok");
    }

    #[tokio::test]
    async fn task_receives_cancellation_token() {
        let mut reg = TaskRegistry::new();
        reg.register("observes_cancel", |_params, _progress, cancel| async move {
            Ok(if cancel.is_cancelled() {
                "cancelled".to_string()
            } else {
                "running".to_string()
            })
        });
        let token = CancellationToken::new();
        token.cancel();
        let task = reg.get("observes_cancel").expect("registered");
        let result = task(serde_json::json!({}), noop_progress(), token).await;
        assert_eq!(result.unwrap(), "cancelled");
    }
}
