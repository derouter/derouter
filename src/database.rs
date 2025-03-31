use std::{path::Path, rc::Rc, sync::LazyLock};

use cleanup::cleanup;
use include_dir::{Dir, include_dir};
use rusqlite::types::Value;
use rusqlite_migration::Migrations;

use crate::dto::{OfferUpdated, ProviderUpdated};
pub use service_connections::create_service_connection;

mod cleanup;
pub mod service_connections;
pub mod service_jobs;

static DB_MIGRATIONS_DIR: Dir =
	include_dir!("$CARGO_MANIFEST_DIR/db/migrations");

static DB_MIGRATIONS: LazyLock<Migrations<'static>> =
	LazyLock::new(|| Migrations::from_directory(&DB_MIGRATIONS_DIR).unwrap());

pub fn open_database(path: &Path) -> eyre::Result<rusqlite::Connection> {
	log::debug!("Opening SQLite connection at {}", path.display());

	let mut conn = rusqlite::Connection::open(path)?;
	rusqlite::vtab::array::load_module(&conn)?;
	DB_MIGRATIONS.to_latest(&mut conn)?;
	cleanup(&conn, true);

	Ok(conn)
}

#[test]
fn db_migrations_test() {
	DB_MIGRATIONS.validate().unwrap();
}

/// Return providers with heartbeat not older than 60 seconds.
pub fn fetch_active_providers(
	database: &rusqlite::Connection,
) -> Vec<ProviderUpdated> {
	let mut stmt = database
		.prepare_cached(
			r#"
				SELECT
					peer_id,                       -- 0
					provider_name,                 -- 1
					provider_teaser,               -- 2
					provider_description,          -- 3
					our_provider_updated_at,       -- 4
					our_latest_heartbeat_timestamp -- 5
				FROM
					peers
				WHERE
					our_provider_updated_at IS NOT NULL AND
					our_latest_heartbeat_timestamp >= datetime('now', '-60 seconds')
			"#,
		)
		.unwrap();

	let providers = stmt
		.query_map([], |row| {
			Ok(ProviderUpdated {
				peer_id: row.get(0)?,
				name: row.get(1)?,
				teaser: row.get(2)?,
				description: row.get(3)?,
				updated_at: row.get(4)?,
				latest_heartbeat_at: row.get(5)?,
			})
		})
		.unwrap();

	providers.map(|p| p.unwrap()).collect()
}

/// Return active offers by these providers.
pub fn fetch_offers(
	database: &rusqlite::Connection,
	provider_peer_ids: &[String],
) -> Vec<OfferUpdated> {
	// See https://github.com/rusqlite/rusqlite/issues/345#issuecomment-1694194547.
	let mut statement = database
		.prepare_cached(
			r#"
				SELECT
					ROWID,            -- 0
					provider_peer_id, -- 1
					protocol_id,      -- 2
					offer_id,         -- 3
					protocol_payload  -- 4
				FROM
					offer_snapshots
				WHERE
					provider_peer_id IN rarray(?1) AND
					active = 1
			"#,
		)
		.unwrap();

	// See https://docs.rs/rusqlite/latest/rusqlite/vtab/array/index.html.
	let provider_ids_param = Rc::new(
		provider_peer_ids
			.iter()
			.map(|p| Value::Text(p.to_string()))
			.collect::<Vec<_>>(),
	);

	let offer_snapshots = statement
		.query_map([provider_ids_param], |row| {
			Ok(OfferUpdated {
				snapshot_id: row.get(0)?,
				provider_peer_id: row.get(1)?,
				protocol_id: row.get(2)?,
				offer_id: row.get(3)?,
				protocol_payload: serde_json::from_str(
					row.get_ref(4)?.as_str().unwrap(),
				)
				.expect("shall deserialize offer payload"),
			})
		})
		.unwrap();

	offer_snapshots.map(|p| p.unwrap()).collect()
}
