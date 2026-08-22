//! Listener, browser, and process-signal shell for local search.

use std::net::SocketAddr;

use color_eyre::eyre::{Result, WrapErr};
use tokio::net::TcpListener;
use tokio::sync::broadcast;

use crate::core::{SearchQuery, SearchSeed, SearchSession};
use crate::server::{self, AppState};

/// Bind a loopback-only listener on a free operating-system assigned port.
pub async fn bind_listener(address: &str) -> std::io::Result<TcpListener> {
    TcpListener::bind(address).await
}

/// Start the local page and stop it after Ctrl-C.
pub async fn run(seed: SearchSeed) -> Result<()> {
    let should_open = seed.open;
    let session = SearchSession::new(SearchQuery::from_seed(&seed));
    let listener = bind_listener("127.0.0.1:0")
        .await
        .wrap_err("cannot bind the local search server")?;
    let address = listener
        .local_addr()
        .wrap_err("cannot read the local search address")?;
    let url = serving_url(address);
    let (shutdown, receiver) = broadcast::channel(1);
    let state = AppState::new(session, shutdown);

    eprintln!("Serving {url}");
    eprintln!("Press Ctrl-C to stop.");
    if should_open && webbrowser::open(&url).is_err() {
        eprintln!("Could not open the default browser. Open {url} manually.");
    }

    let signal = shutdown_signal(state.shutdown_sender());
    let server = server::run(listener, state, receiver);
    let (signal_result, server_result) = tokio::join!(signal, server);
    signal_result?;
    server_result.wrap_err("the local search server stopped with an error")
}

/// Wait for Ctrl-C and notify all local server tasks.
pub async fn shutdown_signal(shutdown: broadcast::Sender<()>) -> Result<()> {
    tokio::signal::ctrl_c()
        .await
        .wrap_err("cannot install the Ctrl-C handler")?;
    let _ = shutdown.send(());
    Ok(())
}

fn serving_url(address: SocketAddr) -> String {
    format!("http://{address}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[tokio::test]
    async fn listener_uses_loopback_and_an_assigned_port() {
        let listener = bind_listener("127.0.0.1:0").await.expect("listener binds");
        let address = listener.local_addr().expect("address is available");
        assert!(address.ip().is_loopback());
        assert_ne!(address.port(), 0);
    }

    #[test]
    fn serving_url_includes_the_assigned_address() {
        let address: SocketAddr = "127.0.0.1:45123".parse().expect("address is valid");
        assert_eq!(serving_url(address), "http://127.0.0.1:45123");
    }
}
