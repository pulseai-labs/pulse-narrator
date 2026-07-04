//! Graceful-shutdown signal helper.
//!
//! The daemon installs [`wait_for_signal`] in a `tokio::select!` against the
//! accept loop. On SIGTERM or SIGINT, the select returns, the accept loop
//! stops accepting, in-flight connection reads drain (bounded by the
//! connection handler's `READ_TIMEOUT`), and the socket file is removed before
//! exit 0 (spec §3 "Graceful shutdown on SIGTERM/SIGINT").

use tokio::signal;

/// Wait for SIGTERM (Unix) or SIGINT / Ctrl-C. Returns on the first signal
/// received. On a non-Unix target, only Ctrl-C is awaited.
pub async fn wait_for_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;
        let sigterm_recv = sigterm.recv();
        let ctrl_c = signal::ctrl_c();
        tokio::pin!(sigterm_recv);
        tokio::pin!(ctrl_c);
        tokio::select! {
            _ = &mut sigterm_recv => tracing::info!("received SIGTERM, initiating shutdown"),
            _ = &mut ctrl_c => tracing::info!("received SIGINT, initiating shutdown"),
        }
    }
    #[cfg(not(unix))]
    {
        signal::ctrl_c().await?;
        tracing::info!("received Ctrl-C, initiating shutdown");
    }
    Ok(())
}
