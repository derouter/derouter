use std::{rc::Rc, str::FromStr};

use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{
	db::Param,
	dto::{Currency, JobRecord},
};

pub fn query_jobs(
	conn: &mut Connection,
	rowid_cursor: Option<i64>,
	protocol_ids: Option<&[String]>,
	provider_peer_ids: Option<&[libp2p::PeerId]>,
	consumer_peer_ids: Option<&[libp2p::PeerId]>,
	limit: i64,
) -> Vec<JobRecord> {
	let mut sql = r#"
    SELECT
      service_jobs.rowid,                        -- #0
      offer_snapshots.provider_peer_id,          -- #1
      service_jobs.consumer_peer_id,             -- #2
      offer_snapshots.rowid,                     -- #3
      service_jobs.currency,                     -- #4
      service_jobs.balance_delta,                -- #5
      service_jobs.public_payload,               -- #6
      service_jobs.private_payload,              -- #7
      service_jobs.reason,                       -- #8
      service_jobs.reason_class,                 -- #9
      service_jobs.created_at_local,             -- #10
      service_jobs.created_at_sync,              -- #11
      service_jobs.completed_at_local,           -- #12
      service_jobs.completed_at_sync,            -- #13
      service_jobs.signature_confirmed_at_local, -- #14
      service_jobs.confirmation_error,           -- #15
      offer_snapshots.protocol_id,               -- #16
			service_jobs.provider_job_id,              -- #17
			offer_snapshots.offer_id,                  -- #18
			offer_snapshots.active,                    -- #19
			offer_snapshots.protocol_payload           -- #20
    FROM
      service_jobs
    JOIN offer_snapshots
      ON offer_snapshots.rowid = service_jobs.offer_snapshot_rowid
    WHERE
      "#
	.to_string();

	let mut params: Vec<Param> = vec![];

	if let Some(rowid_cursor) = rowid_cursor {
		params.push(Param::Single(Value::from(rowid_cursor)));
		sql += &format!("service_jobs.rowid > ?{}", params.len());
	}

	if let Some(protocol_ids) = protocol_ids {
		sql += &format!(
			"{}offer_snapshots.protocol_id IN rarray(?{})",
			if params.is_empty() { "" } else { " AND " },
			params.len() + 1
		);

		params.push(Param::Array(Rc::new(
			protocol_ids
				.iter()
				.map(|p| Value::Text(p.to_string()))
				.collect::<Vec<_>>(),
		)));
	}

	if let Some(provider_peer_ids) = provider_peer_ids {
		sql += &format!(
			"{}offer_snapshots.provider_peer_id IN rarray(?{})",
			if params.is_empty() { "" } else { " AND " },
			params.len() + 1
		);

		params.push(Param::Array(Rc::new(
			provider_peer_ids
				.iter()
				.map(|p| Value::Text(p.to_base58()))
				.collect::<Vec<_>>(),
		)));
	}

	if let Some(consumer_peer_ids) = consumer_peer_ids {
		sql += &format!(
			"{}service_jobs.consumer_peer_id IN rarray(?{})",
			if params.is_empty() { "" } else { " AND " },
			params.len() + 1
		);

		params.push(Param::Array(Rc::new(
			consumer_peer_ids
				.iter()
				.map(|p| Value::Text(p.to_base58()))
				.collect::<Vec<_>>(),
		)));
	}

	sql += &format!("\nLIMIT ?{}", params.len() + 1);
	params.push(Param::Single(Value::from(limit)));

	log::trace!("{} {:?}", sql, params);
	let mut stmt = conn.prepare_cached(&sql).unwrap();

	let jobs = stmt
		.query_map(params_from_iter(params), |row| {
			Ok(JobRecord {
				job_rowid: row.get(0)?,
				provider_peer_id: libp2p::PeerId::from_str(row.get_ref(1)?.as_str()?)
					.unwrap(),
				consumer_peer_id: libp2p::PeerId::from_str(row.get_ref(2)?.as_str()?)
					.unwrap(),
				offer_snapshot_rowid: row.get(3)?,
				currency: Currency::try_from(row.get_ref(4)?.as_i64()?).unwrap(),
				balance_delta: row.get(5)?,
				public_payload: row.get(6)?,
				private_payload: row.get(7)?,
				reason: row.get(8)?,
				reason_class: row.get(9)?,
				created_at_local: row.get(10)?,
				created_at_sync: row.get(11)?,
				completed_at_local: row.get(12)?,
				completed_at_sync: row.get(13)?,
				signature_confirmed_at_local: row.get(14)?,
				confirmation_error: row.get(15)?,
				offer_protocol_id: row.get(16)?,
				provider_job_id: row.get(17)?,
				offer_id: row.get(18)?,
				offer_active: row.get(19)?,
				offer_payload: row.get(20)?,
			})
		})
		.unwrap();

	jobs.map(|job| job.unwrap()).collect()
}

pub struct UnconfirmedJob {
	pub rowid: i64,
	pub provider_peer_id: libp2p::PeerId,
	pub provider_job_id: String,
	pub hash: Vec<u8>,
	pub consumer_signature: Vec<u8>,
}

pub fn query_unconfirmed_jobs(
	conn: &Connection,
	consumer_peer_id: libp2p::PeerId,
) -> Vec<UnconfirmedJob> {
	let mut stmt = conn
		.prepare_cached(
			r#"
				SELECT
					service_jobs.rowid,               -- #0
					offer_snapshots.provider_peer_id, -- #1
					service_jobs.provider_job_id,     -- #2
					service_jobs.hash,                -- #3
					service_jobs.consumer_signature   -- #4
				FROM
					service_jobs
				JOIN offer_snapshots
					ON offer_snapshots.rowid = service_jobs.offer_snapshot_rowid
				WHERE
					service_jobs.consumer_peer_id = ?1 AND
					service_jobs.consumer_signature IS NOT NULL AND
					service_jobs.signature_confirmed_at_local IS NULL
			"#,
		)
		.unwrap();

	stmt
		.query_map([consumer_peer_id.to_base58()], |row| {
			Ok(UnconfirmedJob {
				rowid: row.get(0)?,
				provider_peer_id: libp2p::PeerId::from_str(row.get_ref(1)?.as_str()?)
					.unwrap(),
				provider_job_id: row.get(2)?,
				hash: row.get(3)?,
				consumer_signature: row.get(4)?,
			})
		})
		.unwrap()
		.map(|r| r.unwrap())
		.collect()
}
