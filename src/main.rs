//! Starts the server: `autorumble [config file]`, with `autorumble.toml` as the default file.

use std::process::ExitCode;
use std::sync::Arc;

use autorumble::config::Config;
use autorumble::engine::Engine;
use autorumble::web;

#[tokio::main]
async fn main() -> ExitCode {
    let path = std::env::args().nth(1).unwrap_or_else(|| "autorumble.toml".to_string());
    match run(&path).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("autorumble: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run(path: &str) -> Result<(), String> {
    let config = Config::load(path.as_ref())?;
    let listen = config.listen.clone();
    let engine = Arc::new(Engine::start(config)?);

    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .map_err(|e| format!("cannot listen on {listen}: {e}"))?;
    eprintln!("autorumble: listening on {listen}");

    axum::serve(listener, web::router(engine))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| format!("server error: {e}"))
}

/// Resolves on Ctrl-C or, on Unix, on the SIGTERM sent by systemd.
async fn shutdown_signal() {
    let interrupt = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate()).expect("cannot listen for SIGTERM");
        tokio::select! {
            _ = interrupt => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = interrupt.await;
}
