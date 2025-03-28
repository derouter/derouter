use std::{net::SocketAddr, sync::Arc};
use tokio::{
	io::AsyncReadExt,
	net::{TcpListener, TcpStream},
	task::JoinSet,
};

use crate::state::SharedState;

pub mod consumer;
pub mod provider;

async fn handle_connection(
	mut stream: TcpStream,
	addr: SocketAddr,
	state: Arc<SharedState>,
) {
	match stream.read_u8().await {
		Ok(0u8) => {
			provider::handle_connection(stream, addr, &state).await;
			log::debug!("Closed provider connection at {}", addr)
		}

		Ok(1u8) => {
			consumer::handle_connection(stream, addr, &state).await;
			log::debug!("Closed consumer connection at {}", addr)
		}

		Ok(byte) => {
			log::warn!("Unexpected first byte {}", byte);
		}

		Err(e) => {
			log::error!("{:?}", e);
		}
	}
}

pub async fn run_server(state: Arc<SharedState>) -> eyre::Result<()> {
	let port = state.config.server.port;

	let addr = SocketAddr::from(([127, 0, 0, 1], port));
	let listener = TcpListener::bind(addr).await?;
	log::info!("👂 Listening on tcp://{}", addr);

	let mut connections = JoinSet::new();

	loop {
		tokio::select! {
			Ok((stream, addr)) = listener.accept() => {
				log::debug!("Got new TCP connection from {}", addr);

				// FIXME: When connection panics.
				connections.spawn(handle_connection(
					stream,
					addr,
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
