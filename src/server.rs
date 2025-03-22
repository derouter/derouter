use std::net::SocketAddr;
use tokio::{
	io::AsyncReadExt,
	net::{TcpListener, TcpStream},
	task::JoinSet,
};

use crate::state::SharedState;

mod consumer;
mod provider;

async fn handle_connection(
	mut stream: TcpStream,
	addr: SocketAddr,
	signal: tokio::sync::watch::Receiver<bool>,
	state: SharedState,
) {
	match stream.read_u8().await {
		Ok(0u8) => {
			provider::handle_connection(
				stream,
				addr,
				signal,
				state.all_provided_offers,
			)
			.await;

			log::debug!("Closed provider connection at {}", addr)
		}

		Ok(1u8) => {
			consumer::handle_connection(
				stream,
				addr,
				signal,
				state.consumer_offers_txs,
			)
			.await;

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

pub async fn run(
	port: u16,
	mut signal: tokio::sync::watch::Receiver<bool>,
	state: SharedState,
) -> eyre::Result<()> {
	let addr = SocketAddr::from(([127, 0, 0, 1], port));
	let listener = TcpListener::bind(addr).await?;
	log::info!("👂 Listening on tcp://{}", addr);

	let mut connections = JoinSet::new();

	loop {
		tokio::select! {
			Ok((stream, addr)) = listener.accept() => {
				log::debug!("Got new TCP connection from {}", addr);

				connections.spawn(handle_connection(
					stream,
					addr,
					signal.clone(),
					state.clone()
				));
			},

			_ = signal.changed() => {
				log::info!("🛑 Shutting down...");
				connections.join_all().await;
				log::debug!("✅ All connections closed");
				break;
			}
		}
	}

	Ok(())
}
