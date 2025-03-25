use std::{path::Path, str::FromStr, sync::LazyLock};

use include_dir::{Dir, include_dir};
use rusqlite_migration::Migrations;

use crate::dto::{OfferUpdated, ProviderUpdated};

static DB_MIGRATIONS_DIR: Dir =
	include_dir!("$CARGO_MANIFEST_DIR/db/migrations");

static DB_MIGRATIONS: LazyLock<Migrations<'static>> =
	LazyLock::new(|| Migrations::from_directory(&DB_MIGRATIONS_DIR).unwrap());

pub fn open_database(path: &Path) -> eyre::Result<rusqlite::Connection> {
	log::debug!("Opening SQLite connection at {}", path.display());

	let mut conn = rusqlite::Connection::open(path)?;
	DB_MIGRATIONS.to_latest(&mut conn)?;

	Ok(conn)
}

#[test]
fn db_migrations_test() {
	assert!(DB_MIGRATIONS.validate().is_ok());
}

pub fn fetch_providers(
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
					our_latest_heartbeat_timestamp >= datetime('now', '-30 seconds')
			"#,
		)
		.unwrap();

	let providers = stmt
		.query_map([], |row| {
			Ok(ProviderUpdated {
				peer_id: libp2p::PeerId::from_str(row.get_ref(0)?.as_str().unwrap())
					.unwrap(),
				name: row.get(1)?,
				teaser: row.get(2)?,
				description: row.get(3)?,
				updated_at: row.get(4)?,
				last_heartbeat_at: row.get(5)?,
			})
		})
		.unwrap();

	providers.map(|p| p.unwrap()).collect()
}

pub fn fetch_offers(database: &rusqlite::Connection) -> Vec<OfferUpdated> {
	let mut stmt = database
		.prepare_cached(
			r#"
				SELECT
					provider_peer_id, -- 0
					protocol_id,      -- 1
					offer_id,         -- 2
					protocol_payload, -- 3
				FROM
					offers
				WHERE
					enabled = 1
			"#,
		)
		.unwrap();

	let offers = stmt
		.query_map([], |row| {
			Ok(OfferUpdated {
				provider_peer_id: libp2p::PeerId::from_str(
					row.get_ref(0)?.as_str().unwrap(),
				)
				.unwrap(),
				protocol_id: row.get(1)?,
				offer_id: row.get(2)?,
				_protocol_payload: serde_json::from_str(
					row.get_ref(3)?.as_str().unwrap(),
				)
				.expect("shall deserialize offer payload"),
			})
		})
		.unwrap();

	offers.map(|p| p.unwrap()).collect()
}
