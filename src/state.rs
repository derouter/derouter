use std::{collections::HashMap, fs::create_dir_all, sync::LazyLock};

use directories::ProjectDirs;
use include_dir::{Dir, include_dir};
use rusqlite_migration::Migrations;

use crate::util::{ArcMutex, to_arc_mutex};

static _PERSISTENT_MIGRATIONS_DIR: Dir =
	include_dir!("$CARGO_MANIFEST_DIR/db/persistent/migrations");

static _PERSISTENT_MIGRATIONS: LazyLock<Migrations<'static>> =
	LazyLock::new(|| {
		Migrations::from_directory(&_PERSISTENT_MIGRATIONS_DIR).unwrap()
	});

const PERSISTENT_DB_NAME: &str = "persistent.sqlite";

static TRANSIENT_MIGRATIONS_DIR: Dir =
	include_dir!("$CARGO_MANIFEST_DIR/db/transient/migrations");

static TRANSIENT_MIGRATIONS: LazyLock<Migrations<'static>> =
	LazyLock::new(|| {
		Migrations::from_directory(&TRANSIENT_MIGRATIONS_DIR).unwrap()
	});

const TRANSIENT_DB_NAME: &str = "transient.sqlite";

#[derive(Debug)]
pub struct ProviderOffer {
	pub _protocol_payload: serde_json::Value,
}

#[derive(Clone)]
pub struct OfferNotification {
	pub _protocol: String,
}

/// State shared between all connections.
#[derive(Clone)]
pub struct SharedState {
	/// When a new offer with specific protocol is discovered,
	/// an according event is broadcast (`{ ProtocolId => BroadcastSender }`).
	pub consumer_offers_txs: ArcMutex<
		HashMap<String, tokio::sync::broadcast::Sender<OfferNotification>>,
	>,

	/// An up-to-date collection of offers from all the connected providers
	/// (`{ ProtocolId => { OfferId => Offer } }`).
	pub all_provided_offers:
		ArcMutex<HashMap<String, HashMap<String, ProviderOffer>>>,

	pub _persistent_sqlite_conn: ArcMutex<rusqlite::Connection>,
	pub _transient_sqlite_conn: ArcMutex<rusqlite::Connection>,
}

impl SharedState {
	pub fn new(project_dirs: ProjectDirs) -> eyre::Result<Self> {
		Ok(Self {
			consumer_offers_txs: to_arc_mutex(HashMap::new()),
			all_provided_offers: to_arc_mutex(HashMap::new()),
			_persistent_sqlite_conn: to_arc_mutex(
				create_persistent_sqlite_conn(&project_dirs)?,
			),
			_transient_sqlite_conn: to_arc_mutex(create_transient_sqlite_conn(
				&project_dirs,
			)?),
		})
	}
}

fn create_persistent_sqlite_conn(
	project_dirs: &ProjectDirs,
) -> eyre::Result<rusqlite::Connection> {
	let path = project_dirs.data_dir().to_path_buf();
	create_dir_all(path.clone())?;

	let path = path.join(PERSISTENT_DB_NAME);
	log::debug!("Opening persistent SQLite connection at {}", path.display());

	let conn = rusqlite::Connection::open(path)?;
	// PERSISTENT_MIGRATIONS.to_latest(&mut conn)?;

	Ok(conn)
}

fn create_transient_sqlite_conn(
	project_dirs: &ProjectDirs,
) -> eyre::Result<rusqlite::Connection> {
	let path = project_dirs.cache_dir().to_path_buf();
	create_dir_all(path.clone())?;

	let path = path.join(TRANSIENT_DB_NAME);
	log::debug!("Opening transient SQLite connection at {}", path.display());

	let mut conn = rusqlite::Connection::open(path)?;
	TRANSIENT_MIGRATIONS.to_latest(&mut conn)?;

	Ok(conn)
}

#[test]
fn persistent_migrations_test() {
	// assert!(PERSISTENT_MIGRATIONS.validate().is_ok());
}

#[test]
fn transient_migrations_test() {
	assert!(TRANSIENT_MIGRATIONS.validate().is_ok());
}
