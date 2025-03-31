use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension};

// REFACTOR: Fix `clippy::large_enum_variant`.
#[allow(clippy::large_enum_variant)]
pub enum ConsumerGetCompletedJobResult {
	Ok {
		provider_peer_id: libp2p::PeerId,
		consumer_peer_id: libp2p::PeerId,
		provider_job_id: String,
		job_hash: Vec<u8>,
		consumer_signature: Vec<u8>,
	},

	InvalidJobId,
	NotCompletedYet,
	AlreadyFailed,
	AlreadyConfirmed,
}

/// Get an unconformed job from DB.
pub fn consumer_get_unconfirmed_job(
	database: &mut Connection,
	job_rowid: i64,
) -> ConsumerGetCompletedJobResult {
	struct JobRow {
		provider_peer_id: String,
		consumer_peer_id: String,
		provider_job_id: String,
		reason: Option<String>,
		completed_at_sync: Option<i64>,
		consumer_signature: Option<Vec<u8>>,
		signature_confirmed_at_local: Option<chrono::DateTime<chrono::Utc>>,
		hash: Option<Vec<u8>>,
	}

	let job = database
		.query_row(
			r#"
				SELECT
					offer_snapshots.provider_peer_id,          -- #0
					service_connections.consumer_peer_id,      -- #1
					service_jobs.provider_job_id,              -- #2
					service_jobs.reason,                       -- #3
					service_jobs.completed_at_sync,            -- #4
					service_jobs.consumer_signature,           -- #5
					service_jobs.signature_confirmed_at_local, -- #6
					service_jobs.hash                          -- #7
				FROM service_jobs
				JOIN service_connections
					ON service_connections.rowid = service_jobs.connection_rowid
				JOIN offer_snapshots
					ON offer_snapshots.rowid = service_connections.offer_snapshot_rowid
				WHERE
					service_jobs.rowid = ?1
			"#,
			[job_rowid],
			|row| {
				Ok(JobRow {
					provider_peer_id: row.get(0)?,
					consumer_peer_id: row.get(1)?,
					provider_job_id: row.get(2)?,
					reason: row.get(3)?,
					completed_at_sync: row.get(4)?,
					consumer_signature: row.get(5)?,
					signature_confirmed_at_local: row.get(6)?,
					hash: row.get(7)?,
				})
			},
		)
		.optional()
		.unwrap();

	let job = match job {
		Some(job) => {
			if job.completed_at_sync.is_none() {
				return ConsumerGetCompletedJobResult::NotCompletedYet;
			} else if job.reason.is_some() {
				return ConsumerGetCompletedJobResult::AlreadyFailed;
			} else if job.signature_confirmed_at_local.is_some() {
				return ConsumerGetCompletedJobResult::AlreadyConfirmed;
			}

			job
		}

		None => return ConsumerGetCompletedJobResult::InvalidJobId,
	};

	ConsumerGetCompletedJobResult::Ok {
		consumer_peer_id: libp2p::PeerId::from_str(&job.consumer_peer_id).unwrap(),
		consumer_signature: job.consumer_signature.unwrap(),
		job_hash: job.hash.unwrap(),
		provider_job_id: job.provider_job_id,
		provider_peer_id: libp2p::PeerId::from_str(&job.provider_peer_id).unwrap(),
	}
}
