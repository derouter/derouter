use std::{rc::Rc, str::FromStr};

use libp2p::PeerId;
use rusqlite::{
	Connection, OptionalExtension, params, params_from_iter, types::Value,
};

use crate::dto::OfferSnapshot;

use super::Param;

/// Query offer snapshots by IDs.
pub fn query_offer_snapshots_by_rowid(
	conn: &Connection,
	snapshot_ids: &[i64],
) -> Vec<OfferSnapshot> {
	let mut statement = conn
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
				provider_peer_id: libp2p::PeerId::from_str(row.get_ref(1)?.as_str()?)
					.unwrap(),
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
	conn: &rusqlite::Connection,
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
	let mut stmt = conn.prepare_cached(&sql).unwrap();

	let offer_snapshots = stmt
		.query_map(params_from_iter(params), |row| {
			Ok(OfferSnapshot {
				snapshot_id: row.get(0)?,
				provider_peer_id: libp2p::PeerId::from_str(row.get_ref(1)?.as_str()?)
					.unwrap(),
				protocol_id: row.get(2)?,
				offer_id: row.get(3)?,
				protocol_payload: row.get(4)?,
				active: true,
			})
		})
		.unwrap();

	offer_snapshots.map(|s| s.unwrap()).collect()
}

pub fn find(
	conn: &rusqlite::Connection,
	snapshot_id: i64,
) -> Option<OfferSnapshot> {
	let mut stmt = conn
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
					rowid = ?1
			"#,
		)
		.unwrap();

	stmt
		.query_row([snapshot_id], |row| {
			Ok(OfferSnapshot {
				snapshot_id: row.get(0)?,
				provider_peer_id: libp2p::PeerId::from_str(row.get_ref(1)?.as_str()?)
					.unwrap(),
				protocol_id: row.get(2)?,
				offer_id: row.get(3)?,
				protocol_payload: row.get(4)?,
				active: row.get_ref(5)?.as_i64()? == 1,
			})
		})
		.optional()
		.unwrap()
}

pub fn upsert(
	conn: &mut Connection,
	provider_peer_id: libp2p::PeerId,
	offer_id: &str,
	protocol_id: &str,
	protocol_payload: &str,
) -> i64 {
	let tx = conn.transaction().unwrap();

	let result = {
		let mut ensure_peer_stmt = tx
			.prepare_cached(
				r#"
					INSERT
					INTO peers (peer_id)
					VALUES (?1)
					ON CONFLICT DO NOTHING
				"#,
			)
			.unwrap();

		match ensure_peer_stmt
			.execute(params![provider_peer_id.to_base58()])
			.unwrap()
			.cmp(&0)
		{
			std::cmp::Ordering::Less => unreachable!(),
			std::cmp::Ordering::Equal => {
				log::debug!("Provider peer record already exists in DB")
			}
			std::cmp::Ordering::Greater => {
				log::debug!("Inserted provider peer record")
			}
		}

		let mut upsert_offer_snapshot_stmt = tx
			.prepare_cached(
				r#"
					INSERT
					INTO offer_snapshots (
						provider_peer_id, -- ?1
						protocol_id,      -- ?2
						offer_id,         -- ?3
						active,
						protocol_payload -- ?4
					)
					VALUES (
						?1, ?2, ?3, 1, ?4
					)
					ON CONFLICT DO
						UPDATE SET active = TRUE
					RETURNING
						ROWID
				"#,
			)
			.unwrap();

		upsert_offer_snapshot_stmt
			.query_row(
				params![
					provider_peer_id.to_base58(),
					protocol_id,
					offer_id,
					protocol_payload
				],
				|row| row.get(0),
			)
			.unwrap()
	};

	tx.commit().unwrap();
	result
}
