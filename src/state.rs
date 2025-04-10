use std::{
	collections::{HashMap, HashSet},
	fs::create_dir_all,
	path::PathBuf,
	sync::Arc,
};

use serde::{Deserialize, Serialize};

use crate::{
	UserConfig, UserProviderConfig, db,
	dto::{
		JobRecord, OfferRemoved, OfferSnapshot, ProviderHeartbeat, ProviderRecord,
	},
	p2p, rpc,
	util::{ArcMutex, ArcRw, to_arc_mutex, to_arc_rw},
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
	pub snapshot_rowid: i64,
	pub protocol_payload: String,
}

#[derive(Debug, Clone)]
pub enum RpcEvent {
	OfferRemoved(OfferRemoved),
	OfferUpdated(OfferSnapshot),
	ProviderHeartbeat(ProviderHeartbeat),
	ProviderUpdated(ProviderRecord),
	JobUpdated(Box<JobRecord>),
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

pub struct RpcState {
	/// A broadcast event channel, received by all RPC listeners.
	pub event_tx: tokio::sync::broadcast::Sender<RpcEvent>,
}

#[derive(Default)]
pub struct ConsumerState {
	pub job_completion_notify: Arc<tokio::sync::Notify>,
}

#[derive(Debug)]
pub struct ProviderModuleState {
	/// Channel for outbound requests.
	pub outbound_request_tx:
		tokio::sync::mpsc::Sender<rpc::OutboundRequestEnvelope>,

	pub job_connections_counter: ArcMutex<u64>,

	/// IDs of job connections waiting for the module
	/// to open a Yamux stream for them.
	pub future_job_connections: ArcMutex<HashMap<u64, libp2p::Stream>>,

	/// `ProtocolId` => `OfferId` => [`Offer`](ProvidedOffer).
	/// Synchronized with the [provider](ProviderState)'s `offers_module_map`.
	pub offers: HashMap<String, HashMap<String, ProvidedOffer>>,
}

pub struct ProviderState {
	/// Ever-incrementing module ID counter.
	pub module_ids_counter: u32,

	/// [`ModuleId`](module_ids_counter) => `Module`.
	pub modules: HashMap<u32, ProviderModuleState>,

	/// `ProtocolId` => `OfferId` => [`ModuleId`](module_ids_counter).
	/// Synchronized with the according [module](ProviderModuleState)'s `offers`.
	pub offers_module_map: HashMap<String, HashMap<String, u32>>,

	/// When the provider data has been last time updated at.
	pub last_updated_at: chrono::DateTime<chrono::Utc>,
}

pub struct FoundProvidedOffer<'a> {
	pub module: &'a ProviderModuleState,
	pub provided: ProvidedOffer,
}

impl ProviderState {
	pub fn find_offer(
		&self,
		protocol_id: &str,
		offer_id: &str,
	) -> Option<FoundProvidedOffer> {
		match self.offers_module_map.get(protocol_id) {
			Some(module_ids_by_protocol_id) => {
				let module_id = module_ids_by_protocol_id.get(offer_id);

				module_id.map(|module_id| FoundProvidedOffer {
					module: self.modules.get(module_id).unwrap(),
					provided: self
						.modules
						.get(module_id)
						.expect("provider module offers to be in sync")
						.offers
						.get(protocol_id)
						.expect("provider module offers to be in sync")
						.get(offer_id)
						.expect("provider module offers to be in sync")
						.clone(),
				})
			}
			None => None,
		}
	}
}

pub struct P2pState {
	/// Set when P2P is running.
	pub keypair: libp2p::identity::Keypair,

	/// Set of currently discovered peers on the network.
	pub peers: ArcRw<HashMap<libp2p::PeerId, HashSet<libp2p::Multiaddr>>>,

	/// Notify when `peers` changes.
	pub peers_notify: Arc<tokio::sync::Notify>,

	/// Channel for outbound ReqRes requests.
	#[allow(dead_code)]
	pub reqres_request_tx:
		tokio::sync::mpsc::Sender<p2p::reqres::OutboundRequestEnvelope>,

	/// Channel for outbound Stream requests.
	pub stream_request_tx:
		tokio::sync::mpsc::Sender<p2p::stream::OutboundStreamRequest>,
}

pub struct SharedState {
	pub config: Config,
	pub shutdown_token: tokio_util::sync::CancellationToken,
	pub rpc: ArcMutex<RpcState>,
	pub consumer: ConsumerState,
	pub provider: ArcMutex<ProviderState>,
	pub db: ArcMutex<rusqlite::Connection>,
	pub p2p: ArcMutex<P2pState>,
}

impl SharedState {
	pub fn new(
		user_config: &Option<UserConfig>,
		shutdown_token: tokio_util::sync::CancellationToken,
		p2p_reqres_request_tx: tokio::sync::mpsc::Sender<
			p2p::reqres::OutboundRequestEnvelope,
		>,
		p2p_stream_request_tx: tokio::sync::mpsc::Sender<
			p2p::stream::OutboundStreamRequest,
		>,
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

		let database = db::open(&database_path)?;

		let keypair_path =
			match user_config.as_ref().and_then(|c| c.keypair_path.clone()) {
				Some(path) => path,
				None => data_dir.join(DEFAULT_KEYPAIR_FILE_NAME),
			};

		let keypair = p2p::read_or_create_keypair(&keypair_path)?;

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
			keypair,
			peers: to_arc_rw(Default::default()),
			peers_notify: Default::default(),
			reqres_request_tx: p2p_reqres_request_tx,
			stream_request_tx: p2p_stream_request_tx,
		};

		Ok(Self {
			config,
			shutdown_token,

			rpc: to_arc_mutex(RpcState {
				event_tx: tokio::sync::broadcast::channel(32).0,
			}),

			consumer: ConsumerState::default(),

			provider: to_arc_mutex(ProviderState {
				module_ids_counter: 0,
				modules: HashMap::new(),
				offers_module_map: HashMap::new(),
				last_updated_at: chrono::Utc::now(),
			}),

			db: to_arc_mutex(database),
			p2p: to_arc_mutex(p2p_state),
		})
	}
}
