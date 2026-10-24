//! Per-job cancellation.
//!
//! A job is cancelled in one of two ways depending on where it is in its
//! lifecycle:
//!
//! * **PENDING** (still queued) — the controller flips the `job_status` row to
//!   `CANCELLED` with a conditional `UPDATE ... WHERE status = 'PENDING'`, so
//!   `claim_jobs` can never pick it up. No token exists yet.
//! * **RUNNING** — the worker registers a [`CancellationToken`] for the job in
//!   [`JobCancellation`]; the controller marks the row `CANCELLED` and fires
//!   the token, and the task stops at its next cancellation check.
//!
//! Tasks signal an early stop with [`JobCancelled`], which the worker treats as
//! a terminal `CANCELLED` state rather than a failure.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio_util::sync::CancellationToken;

/// Registry of cancellation tokens for in-flight jobs, keyed by job id.
///
/// Held in Tauri managed state so the `cancel_job` command can reach the token
/// belonging to a job the worker is currently running.
#[derive(Clone, Default)]
pub struct JobCancellation(Arc<Mutex<HashMap<String, CancellationToken>>>);

impl JobCancellation {
    pub fn new() -> Self {
        Self::default()
    }

    /// Store the token for `job_id`, replacing any stale entry.
    pub fn register(&self, job_id: &str, token: CancellationToken) {
        if let Ok(mut map) = self.0.lock() {
            map.insert(job_id.to_string(), token);
        }
    }

    /// Drop the token once the job reached a terminal state.
    pub fn remove(&self, job_id: &str) {
        if let Ok(mut map) = self.0.lock() {
            map.remove(job_id);
        }
    }

    /// Fire the token for `job_id`. Returns `false` when no token is
    /// registered, i.e. the job is not running (or has not registered yet).
    pub fn cancel(&self, job_id: &str) -> bool {
        let token = self
            .0
            .lock()
            .ok()
            .and_then(|map| map.get(job_id).cloned());
        match token {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }
}

/// Error returned by a task that stopped early because its job was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobCancelled;

impl std::fmt::Display for JobCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("job cancelled")
    }
}

impl std::error::Error for JobCancelled {}

/// True when `err` (or anything in its context chain) is a [`JobCancelled`].
pub fn is_job_cancelled(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| cause.is::<JobCancelled>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_fires_registered_token() {
        let reg = JobCancellation::new();
        let token = CancellationToken::new();
        reg.register("job-1", token.clone());

        assert!(!token.is_cancelled());
        assert!(reg.cancel("job-1"));
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancel_unknown_job_reports_false() {
        let reg = JobCancellation::new();
        assert!(!reg.cancel("missing"));
    }

    #[test]
    fn remove_drops_token() {
        let reg = JobCancellation::new();
        let token = CancellationToken::new();
        reg.register("job-2", token.clone());
        reg.remove("job-2");
        assert!(!reg.cancel("job-2"));
        assert!(!token.is_cancelled());
    }

    #[test]
    fn is_job_cancelled_matches_direct_and_contextual_errors() {
        let direct = anyhow::Error::new(JobCancelled);
        assert!(is_job_cancelled(&direct));

        let wrapped = anyhow::Error::new(JobCancelled).context("embedding zip");
        assert!(is_job_cancelled(&wrapped));

        let other = anyhow::anyhow!("some other failure");
        assert!(!is_job_cancelled(&other));
    }

    #[tokio::test]
    async fn cancelled_token_short_circuits_select() {
        // Mirrors how tasks await cancellation: the branch resolves as soon as
        // the token fires.
        let token = CancellationToken::new();
        token.cancel();
        tokio::select! {
            _ = token.cancelled() => {}
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => panic!("token ignored"),
        }
    }
}
