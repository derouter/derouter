use rusqlite::{Connection, params};

use crate::dto::Currency;

/// Create new service job entry.
/// Returns `job_rowid`.
pub fn create_job(
	conn: &mut Connection,
	offer_snapshot_rowid: i64,
	consumer_peer_id: libp2p::PeerId,
	currency: Currency,
	job_args: Option<&str>,
	provider_job_id: &str,
	created_at_sync: i64,
) -> i64 {
	let mut insert_job_stmt = conn
		.prepare_cached(
			r#"
				INSERT
				INTO service_jobs (
					offer_snapshot_rowid, -- ?1
					consumer_peer_id,     -- ?2
					currency,             -- ?3
					job_args,             -- ?4
					provider_job_id,      -- ?5
					created_at_sync       -- ?6
				)
				VALUES (?1, ?2, ?3, ?4, ?5, ?6)
			"#,
		)
		.unwrap();

	insert_job_stmt
		.execute(params![
			offer_snapshot_rowid,
			consumer_peer_id.to_base58(),
			currency as i64,
			job_args,
			provider_job_id,
			created_at_sync
		])
		.unwrap();

	conn.last_insert_rowid()
}
