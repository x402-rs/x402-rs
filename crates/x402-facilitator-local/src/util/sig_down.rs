//! Graceful shutdown signal handling.
//!
//! This module provides [`SigDown`], a utility for handling Unix shutdown
//! signals (SIGTERM and SIGINT) and coordinating graceful shutdown across
//! multiple subsystems using cancellation tokens.
//!
//! # Example
//!
//! ```ignore
//! use x402::util::SigDown;
//!
//! let sig_down = SigDown::try_new()?;
//! let token = sig_down.cancellation_token();
//!
//! // Pass token to subsystems
//! tokio::spawn(async move {
//!     token.cancelled().await;
//!     println!("Shutting down...");
//! });
//!
//! // Wait for shutdown signal
//! sig_down.recv().await;
//! ```

use tokio::signal::unix::SignalKind;
use tokio::signal::unix::signal;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// Handles graceful shutdown on SIGTERM and SIGINT signals.
///
/// Spawns a background task that listens for shutdown signals and triggers
/// a cancellation token when received.
pub struct SigDown {
    task_tracker: TaskTracker,
    cancellation_token: CancellationToken,
}

impl SigDown {
    /// Creates a new signal handler.
    ///
    /// Returns an error if signal registration fails.
    pub fn try_new() -> Result<Self, std::io::Error> {
        let mut sigterm = signal(SignalKind::terminate())?;
        let mut sigint = signal(SignalKind::interrupt())?;
        let inner = CancellationToken::new();
        let outer = inner.clone();
        let task_tracker = TaskTracker::new();
        task_tracker.spawn(async move {
            tokio::select! {
                _ = sigterm.recv() => {
                    inner.cancel();
                },
                _ = sigint.recv() => {
                    inner.cancel();
                }
            }
        });
        task_tracker.close();
        Ok(Self {
            task_tracker,
            cancellation_token: outer,
        })
    }

    /// Returns a clone of the cancellation token for distributing to subsystems.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }

    /// Waits for a shutdown signal and ensures the signal handler task completes.
    #[allow(dead_code)]
    pub async fn recv(&self) {
        self.cancellation_token.cancelled().await;
        self.task_tracker.wait().await;
    }
}
