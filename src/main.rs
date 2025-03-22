use clap::Parser as _;
use state::SharedState;

mod logger;
mod server;
mod state;
mod util;

// We want the default project name to be `org.derouter`.
// TODO: Allow to override this (e.g. for a Tauri app).
const APP_QUALIFIER: &str = "org";
const APP_ORG: &str = "";
const APP_NAME: &str = "derouter";

#[derive(clap::Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
	/// HTTP server port.
	#[arg(long, short, default_value = "4269")]
	port: u16,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
	let _logger = logger::setup_logger()?;

	let args = Args::parse();
	log::debug!("{:?}", args);

	let project_dirs =
		directories::ProjectDirs::from(APP_QUALIFIER, APP_ORG, APP_NAME)
			.expect("should get project directories");

	let state = SharedState::new(project_dirs)?;

	let (signal_tx, signal_rx) = tokio::sync::watch::channel(false);

	let server_handle = tokio::spawn(async move {
		if let Err(e) = server::run(args.port, signal_rx, state.clone()).await {
			log::error!("{}", e)
		}
	});

	tokio::select! {
		r = tokio::signal::ctrl_c() => {
			r.expect("should set CTRL+C signal handler");
			signal_tx.send(true).expect("should send signal_tx");
		},

		r = server_handle => {
			r.unwrap();
		}
	}

	signal_tx.closed().await;

	Ok(())
}
