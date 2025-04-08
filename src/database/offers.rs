use std::rc::Rc;

use libp2p::PeerId;
use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::dto::OfferSnapshot;

use super::Param;

/// Query offer snapshots by IDs.
pub fn query_offer_snapshots_by_rowid(
	database: &Connection,
	snapshot_ids: &[i64],
) -> Vec<OfferSnapshot> {
	let mut statement = database
		.prepare_cached(
			r#"
        SELECT
          rowid,            -- #0
          provider_peer_id, -- #1
          protocol_id,      -- #2
          offer_id,         -- #3
          protocol_payload, -- #4
          active            -- #5
        FROM
          offer_snapshots
        WHERE
          rowid IN rarray(?1)
      "#,
		)
		.unwrap();

	let snapshot_ids = Rc::new(
		snapshot_ids
			.iter()
			.map(|p| Value::Text(p.to_string()))
			.collect::<Vec<_>>(),
	);

	let offer_snapshots = statement
		.query_map(params![snapshot_ids], |row| {
			Ok(OfferSnapshot {
				snapshot_id: row.get(0)?,
				provider_peer_id: row.get(1)?,
				protocol_id: row.get(2)?,
				offer_id: row.get(3)?,
				protocol_payload: row.get(4)?,
				active: row.get_ref(5)?.as_i64()? == 1,
			})
		})
		.unwrap();

	offer_snapshots.map(|s| s.unwrap()).collect()
}

/// Query *active* offers.
pub fn query_active_offers(
	database: &rusqlite::Connection,
	protocol_ids: Option<&[String]>,
	provider_peer_ids: Option<&[PeerId]>,
) -> Vec<OfferSnapshot> {
	let mut sql = r#"
    SELECT
      rowid,            -- #0
      provider_peer_id, -- #1
      protocol_id,      -- #2
      offer_id,         -- #3
      protocol_payload  -- #4
    FROM
      offer_snapshots
    WHERE
      active = 1
  "#
	.to_string();

	let mut params: Vec<Param> = vec![];

	if let Some(protocol_ids) = protocol_ids {
		params.push(Param::Array(Rc::new(
			protocol_ids
				.iter()
				.map(|p| Value::Text(p.to_string()))
				.collect::<Vec<_>>(),
		)));

		sql += &format!(" AND protocol_id IN rarray(?{})", params.len());
	}

	if let Some(provider_peer_ids) = provider_peer_ids {
		params.push(Param::Array(Rc::new(
			provider_peer_ids
				.iter()
				.map(|p| Value::Text(p.to_base58()))
				.collect::<Vec<_>>(),
		)));

		sql += &format!(" AND provider_peer_id IN rarray(?{})", params.len());
	}

	log::trace!("{} {:?}", sql, params);
	let mut stmt = database.prepare_cached(&sql).unwrap();

	let offer_snapshots = stmt
		.query_map(params_from_iter(params), |row| {
			Ok(OfferSnapshot {
				snapshot_id: row.get(0)?,
				provider_peer_id: row.get(1)?,
				protocol_id: row.get(2)?,
				offer_id: row.get(3)?,
				protocol_payload: row.get(4)?,
				active: true,
			})
		})
		.unwrap();

	offer_snapshots.map(|s| s.unwrap()).collect()
}
