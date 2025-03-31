use std::{collections::HashMap, fs::create_dir_all, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
	UserConfig, UserProviderConfig,
	database::open_database,
	dto::{OfferRemoved, OfferUpdated, ProviderHeartbeat, ProviderUpdated},
	p2p::{OutboundReqResRequestEnvelope, OutboundStreamRequest},
	server,
	util::{ArcMutex, to_arc_mutex},
};

// We want the default project name to be `org.derouter`.
// TODO: Allow to override these (e.g. for a Tauri app).
const PROJECT_QUALIFIER: &str = "org";
const PROJECT_ORG: &str = "";
const PROJECT_NAME: &str = "derouter";

const DEFAULT_DB_NAME: &str = "db.sqlite";
const DEFAULT_KEYPAIR_FILE_NAME: &str = "keypair.bin";
const DEFAULT_SERVER_PORT: u16 = 4269;

#[derive(Clone, Debug)]
pub struct ProvidedOffer {
	pub provider_id: String,

	/// It may or may not be stored into DB yet.
	pub snapshot_rowid: Option<i64>,

	pub protocol_payload: serde_json::Value,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum ConsumerNotification {
	OfferRemoved(OfferRemoved),
	OfferUpdated(OfferUpdated),
	ProviderHeartbeat(ProviderHeartbeat),
	ProviderUpdated(ProviderUpdated),
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ProviderConfig {
	pub name: Option<String>,
	pub teaser: Option<String>,
	pub description: Option<String>,
}

impl From<Option<&UserProviderConfig>> for ProviderConfig {
	fn from(value: Option<&UserProviderConfig>) -> Self {
		Self {
			name: value.as_ref().and_then(|c| c.name.clone()),
			teaser: value.as_ref().and_then(|c| c.teaser.clone()),
			description: value.as_ref().and_then(|c| c.description.clone()),
		}
	}
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ServerConfig {
	pub port: u16,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
	pub database_path: PathBuf,
	pub keypair_path: PathBuf,
	pub data_dir: PathBuf,
	pub server: ServerConfig,
	pub provider: ProviderConfig,
}

pub struct ConsumerState {
	/// A broadcast notification channel, received by every consumer module.
	pub notification_tx: tokio::sync::broadcast::Sender<ConsumerNotification>,
}

pub struct ProviderOutboundRequestEnvelope {
	pub frame_data: server::provider::rpc::OutboundRequestFrameData,
	pub response_tx: tokio::sync::oneshot::Sender<
		server::provider::rpc::InboundResponseFrameData,
	>,
}

pub struct ProviderModuleState {
	/// Channel for outbound requests.
	pub outbound_request_tx:
		tokio::sync::mpsc::Sender<ProviderOutboundRequestEnvelope>,

	/// ROWIDs of provider service connections waiting
	/// for the module to open a Yamux stream for it.
	pub future_service_connections: ArcMutex<HashMap<i64, libp2p::Stream>>,
}

pub struct ProviderState {
	pub modules: HashMap<String, ProviderModuleState>,

	/// A map of actual offers, defined by the connected provider modules
	/// (`{ ProtocolId => { OfferId => Offer } }`).
	// REFACTOR: Move to `ProviderStateModule`.
	pub offers: HashMap<String, HashMap<String, ProvidedOffer>>,

	/// When the provider data has been last time updated at.
	pub last_updated_at: chrono::DateTime<chrono::Utc>,
}

pub struct P2pState {
	/// Set when P2P is running.
	keypair: Option<libp2p::identity::Keypair>,

	/// Channel for outbound ReqRes requests.
	#[allow(dead_code)]
	pub reqres_request_tx:
		tokio::sync::mpsc::Sender<OutboundReqResRequestEnvelope>,

	/// Channel for outbound stream requests.
	pub stream_request_tx: tokio::sync::mpsc::Sender<OutboundStreamRequest>,
}

impl P2pState {
	pub fn set_keypair(&mut self, keypair: libp2p::identity::Keypair) {
		self.keypair = Some(keypair);
	}

	pub fn sign(
		&self,
		msg: &[u8],
	) -> Option<Result<Vec<u8>, libp2p::identity::SigningError>> {
		self.keypair.as_ref().map(|k| k.sign(msg))
	}

	pub fn public_key(&self) -> Option<libp2p::identity::PublicKey> {
		self.keypair.as_ref().map(|k| k.public())
	}
}

pub struct SharedState {
	pub config: Config,
	pub shutdown_token: tokio_util::sync::CancellationToken,
	pub consumer: ArcMutex<ConsumerState>,
	pub provider: ArcMutex<ProviderState>,
	pub database: ArcMutex<rusqlite::Connection>,
	pub p2p: ArcMutex<P2pState>,
}

impl SharedState {
	pub fn new(
		user_config: &Option<UserConfig>,
		shutdown_token: tokio_util::sync::CancellationToken,
		p2p_reqres_request_tx: tokio::sync::mpsc::Sender<
			OutboundReqResRequestEnvelope,
		>,
		p2p_stream_request_tx: tokio::sync::mpsc::Sender<OutboundStreamRequest>,
	) -> eyre::Result<Self> {
		let data_dir = match user_config.as_ref().and_then(|c| c.data_dir.clone()) {
			Some(path) => path,
			None => {
				let project_dirs = directories::ProjectDirs::from(
					PROJECT_QUALIFIER,
					PROJECT_ORG,
					PROJECT_NAME,
				)
				.expect("should get project directories");
				let path = project_dirs.data_local_dir().to_path_buf();
				create_dir_all(&path)?;
				path
			}
		};

		let database_path =
			match user_config.as_ref().and_then(|c| c.database_path.clone()) {
				Some(path) => path,
				None => data_dir.join(DEFAULT_DB_NAME),
			};

		let database = open_database(&database_path)?;

		let keypair_path =
			match user_config.as_ref().and_then(|c| c.keypair_path.clone()) {
				Some(path) => path,
				None => data_dir.join(DEFAULT_KEYPAIR_FILE_NAME),
			};

		let server_port = user_config
			.as_ref()
			.and_then(|c| c.server.as_ref().and_then(|s| s.port))
			.unwrap_or(DEFAULT_SERVER_PORT);

		let server_config = ServerConfig { port: server_port };

		let provider_config = ProviderConfig::from(
			user_config.as_ref().and_then(|c| c.provider.as_ref()),
		);

		let config = Config {
			database_path,
			keypair_path,
			data_dir,
			server: server_config,
			provider: provider_config,
		};

		let p2p_state = P2pState {
			keypair: None,
			reqres_request_tx: p2p_reqres_request_tx,
			stream_request_tx: p2p_stream_request_tx,
		};

		Ok(Self {
			config,
			shutdown_token,

			consumer: to_arc_mutex(ConsumerState {
				notification_tx: tokio::sync::broadcast::channel(32).0,
			}),

			provider: to_arc_mutex(ProviderState {
				modules: HashMap::new(),
				offers: HashMap::new(),
				last_updated_at: chrono::Utc::now(),
			}),

			database: to_arc_mutex(database),
			p2p: to_arc_mutex(p2p_state),
		})
	}
}
