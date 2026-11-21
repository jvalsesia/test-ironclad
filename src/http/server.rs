//! Server lifecycle: signal handling and graceful shutdown.

use std::future::{Future, IntoFuture};
use std::io;
use std::time::Duration;

use axum::Router;
use tokio::net::TcpListener;
use tracing::{info, warn};

/// How long in-flight requests may keep running after a shutdown signal.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

/// Completes when SIGINT or SIGTERM is received.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            // The handler could not be installed; never resolve from here.
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    info!("shutdown signal received");
}

/// Serves `app` until `shutdown` completes, then stops accepting
/// connections and waits up to `grace` for in-flight requests. After the
/// deadline the remaining requests are dropped and this returns `Ok`.
pub async fn serve<F>(
    listener: TcpListener,
    app: Router,
    grace: Duration,
    shutdown: F,
) -> io::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let (signalled_tx, signalled_rx) = tokio::sync::oneshot::channel::<()>();
    let graceful = async move {
        shutdown.await;
        // The receiver only goes away once serving has already finished.
        let _ = signalled_tx.send(());
    };

    let server = axum::serve(listener, app)
        .with_graceful_shutdown(graceful)
        .into_future();
    tokio::pin!(server);

    let deadline = async {
        if signalled_rx.await.is_ok() {
            tokio::time::sleep(grace).await;
        } else {
            std::future::pending::<()>().await;
        }
    };

    tokio::select! {
        result = &mut server => result,
        () = deadline => {
            warn!("shutdown deadline exceeded; dropping in-flight requests");
            Ok(())
        }
    }
}
