use std::{net::SocketAddr, sync::Arc};
use tokio::{net::TcpListener, task::JoinSet};

use crate::{rpc::connection::handle_connection, state::SharedState};

pub async fn run(state: Arc<SharedState>) -> eyre::Result<()> {
	let port = state.config.server.port;

	let addr = SocketAddr::from(([127, 0, 0, 1], port));
	let listener = TcpListener::bind(addr).await?;
	log::info!("👂 Listening on tcp://{}", addr);

	let mut connections = JoinSet::new();

	loop {
		tokio::select! {
			Ok((stream, addr)) = listener.accept() => {
				log::debug!("Got new TCP connection from {}", addr);

				// BUG: When connection panics.
				connections.spawn(handle_connection(
					stream,
					state.clone()
				));
			},

			_ = state.shutdown_token.cancelled() => {
				log::info!("🛑 Shutting down...");
				connections.join_all().await;
				log::debug!("✅ All connections closed");
				break;
			}
		}
	}

	Ok(())
}
