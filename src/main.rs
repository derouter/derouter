use std::path::PathBuf;

use clap::Parser as _;
use serde::{Deserialize, Serialize};
use state::SharedState;
use util::to_arc;

mod db;
mod dto;
mod logger;
mod p2p;
mod rpc;
mod state;
mod util;

#[derive(clap::Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
	/// Path to configuration file (`.json`, `.json5` or `.jsonc`).
	#[arg(long, short)]
	config: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserProviderConfig {
	pub name: Option<String>,
	pub teaser: Option<String>,
	pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserServerConfig {
	pub port: Option<u16>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserConfig {
	/// An optional path to the database file,
	/// otherwise [`data_dir`] + `"db.sqlite"`.
	pub database_path: Option<PathBuf>,

	/// An optional path to the P2P keypair file,
	/// otherwise [`data_dir`] + `"keypair.bin"`.
	pub keypair_path: Option<PathBuf>,

	/// An optional path to the data directory,
	/// otherwise platform-specific.
	pub data_dir: Option<PathBuf>,

	pub server: Option<UserServerConfig>,
	pub provider: Option<UserProviderConfig>,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
	let _logger = logger::setup_logger()?;

	let args = Args::parse();
	log::debug!("{:?}", args);

	let config = if let Some(config_path) = args.config {
		let config_string = std::fs::read_to_string(config_path)?;
		Some(json5::from_str::<UserConfig>(&config_string)?)
	} else {
		None
	};
	log::debug!("{:?}", config);

	let shutdown_token = tokio_util::sync::CancellationToken::new();

	let (p2p_reqres_request_tx, p2p_reqres_request_rx) =
		tokio::sync::mpsc::channel::<p2p::reqres::OutboundRequestEnvelope>(32);

	let (p2p_stream_request_tx, p2p_stream_request_rx) =
		tokio::sync::mpsc::channel::<p2p::stream::OutboundStreamRequest>(32);

	let state = to_arc(SharedState::new(
		&config,
		shutdown_token.child_token(),
		p2p_reqres_request_tx,
		p2p_stream_request_tx,
	)?);

	let shutdown_token_clone = shutdown_token.clone();
	tokio::spawn(async move {
		tokio::signal::ctrl_c().await.unwrap();
		log::info!("🛑 Sending shutdown signal (PID {})", std::process::id());
		shutdown_token_clone.cancel();
	});

	let tracker = tokio_util::task::TaskTracker::new();

	let state_clone = state.clone();
	let shutdown_token_clone = shutdown_token.clone();

	let p2p_handle = tracker.spawn(async move {
		if let Err(e) =
			p2p::run(state_clone, p2p_reqres_request_rx, p2p_stream_request_rx).await
		{
			log::error!("{:?}", e);
			shutdown_token_clone.cancel();
		}
	});

	let state_clone = state.clone();
	let shutdown_token_clone = shutdown_token.clone();

	let server_handle = tracker.spawn(async move {
		if let Err(e) = rpc::server::run(state_clone).await {
			log::error!("{:?}", e);
			shutdown_token_clone.cancel();
		}
	});

	tracker.close();

	match tokio::try_join!(p2p_handle, server_handle) {
		Ok(_) => {
			log::debug!("✨ Clean exit")
		}
		Err(e) => {
			log::error!("{}", e);
			shutdown_token.cancel();
		}
	}

	tracker.wait().await;

	Ok(())
}
