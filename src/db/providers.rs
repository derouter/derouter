use std::{rc::Rc, str::FromStr, time::Duration};

use rusqlite::{Connection, types::Value};

use crate::dto::ProviderRecord;

/// Query providers by peer IDs.
pub fn query_providers_by_peer_id(
	conn: &Connection,
	provider_peer_ids: &[libp2p::PeerId],
) -> Vec<ProviderRecord> {
	let mut stmt = conn
		.prepare_cached(
			r#"
				SELECT
					peer_id,                       -- #0
					provider_name,                 -- #1
					provider_teaser,               -- #2
					provider_description,          -- #3
					our_provider_updated_at,       -- #4
					our_latest_heartbeat_timestamp -- #5
				FROM
					peers
				WHERE
          peer_id IN rarray(?1)
			"#,
		)
		.unwrap();

	let provider_peer_ids = Rc::new(
		provider_peer_ids
			.iter()
			.map(|p| Value::Text(p.to_base58()))
			.collect::<Vec<_>>(),
	);

	let providers = stmt
		.query_map([provider_peer_ids], |row| {
			Ok(ProviderRecord {
				peer_id: libp2p::PeerId::from_str(row.get_ref(0)?.as_str()?).unwrap(),
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

/// Return peer IDs of providers with recent heartbeat.
pub fn query_active_providers(
	conn: &Connection,
	heartbeat_timeout: Duration,
) -> Vec<libp2p::PeerId> {
	let mut stmt = conn
		.prepare_cached(
			r#"
				SELECT
					peer_id
				FROM
					peers
				WHERE
					our_provider_updated_at IS NOT NULL AND
					our_latest_heartbeat_timestamp >= datetime('now', ?1)
			"#,
		)
		.unwrap();

	let interval = format!("-{} seconds", heartbeat_timeout.as_secs());
	let peer_ids = stmt
		.query_map([interval], |row| {
			Ok(libp2p::PeerId::from_str(row.get_ref(0)?.as_str()?).unwrap())
		})
		.unwrap();

	peer_ids.map(|p| p.unwrap()).collect()
}
