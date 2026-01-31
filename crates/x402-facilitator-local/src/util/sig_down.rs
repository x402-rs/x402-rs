//! Graceful shutdown signal handling.
//!
//! This module provides [`SigDown`], a utility for handling Unix shutdown
//! signals (SIGTERM and SIGINT) and coordinating graceful shutdown across
//! multiple subsystems using cancellation tokens.
//!
//! # Example
//!
//! ```ignore
//! use x402_facilitator_local::util::SigDown;
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
//!
//! # Architecture
//!
//! [`SigDown`] spawns a background task that listens for Unix signals (SIGTERM and SIGINT).
//! When a signal is received, it triggers a [`CancellationToken`] that can be distributed
//! to multiple subsystems. This allows for coordinated graceful shutdown where all
//! components can clean up resources before the application exits.
//!
//! The [`TaskTracker`] is used to ensure the signal handler task completes before
//! the application exits.

use tokio::signal::unix::SignalKind;
use tokio::signal::unix::signal;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// Handles graceful shutdown on SIGTERM and SIGINT signals.
///
/// Spawns a background task that listens for shutdown signals and triggers
/// a cancellation token when received.
///
/// # Example
///
/// ```ignore
/// use x402_facilitator_local::util::SigDown;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let sig_down = SigDown::try_new()?;
///     let token = sig_down.cancellation_token();
///
///     // Use token for graceful shutdown in axum
///     let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
///     axum::serve(listener, app)
///         .with_graceful_shutdown(async move { token.cancelled().await })
///         .await?;
///
///     Ok(())
/// }
/// ```
pub struct SigDown {
    task_tracker: TaskTracker,
    cancellation_token: CancellationToken,
}

impl SigDown {
    /// Creates a new signal handler.
    ///
    /// Returns an error if signal registration fails (e.g., if the platform
    /// does not support Unix signals).
    ///
    /// # Errors
    ///
    /// Returns an [`std::io::Error`] if signal registration fails.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use x402_facilitator_local::util::SigDown;
    ///
    /// let sig_down = SigDown::try_new()?;
    /// let token = sig_down.cancellation_token();
    /// ```
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
    ///
    /// The token can be passed to multiple subsystems. When a shutdown signal is received,
    /// all clones of the token will be cancelled simultaneously.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use x402_facilitator_local::util::SigDown;
    ///
    /// let sig_down = SigDown::try_new()?;
    /// let token = sig_down.cancellation_token();
    ///
    /// // Pass token to multiple subsystems
    /// let token2 = token.clone();
    /// tokio::spawn(async move {
    ///     token2.cancelled().await;
    ///     println!("Subsystem 2 shutting down...");
    /// });
    /// ```
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }

    /// Waits for a shutdown signal and ensures the signal handler task completes.
    ///
    /// This method blocks until either SIGTERM or SIGINT is received, then waits
    /// for the signal handler task to complete. This ensures clean shutdown
    /// without leaving background tasks running.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use x402_facilitator_local::util::SigDown;
    ///
    /// let sig_down = SigDown::try_new()?;
    ///
    /// // Wait for shutdown signal
    /// sig_down.recv().await;
    /// println!("Shutdown complete");
    /// ```
    #[allow(dead_code)]
    pub async fn recv(&self) {
        self.cancellation_token.cancelled().await;
        self.task_tracker.wait().await;
    }
}
