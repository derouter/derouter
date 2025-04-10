use alloy_primitives::U256;
use eyre::eyre;
use sha2::Digest;

use crate::dto::Currency;

pub mod consumer;
pub mod create;
pub mod fail;
pub mod get;
pub mod provider;
pub mod query_all;
pub mod set_confirmation_error;

pub use query_all::query_unconfirmed_jobs;

struct JobHashingPayload<'a> {
	consumer_peer_id: libp2p::PeerId,
	provider_peer_id: libp2p::PeerId,
	protocol_id: &'a str,
	offer_payload: &'a str,
	currency: &'a str,
	provider_job_id: &'a str,
	job_created_at_sync: i64,
	job_public_payload: &'a str,
	job_completed_at_sync: i64,
	balance_delta: Option<&'a str>,
}

fn hash_job(job: &JobHashingPayload) -> Vec<u8> {
	let mut hasher = sha2::Sha256::new();

	sha2::Digest::update(&mut hasher, job.consumer_peer_id.to_bytes());
	sha2::Digest::update(&mut hasher, job.provider_peer_id.to_bytes());
	sha2::Digest::update(&mut hasher, job.protocol_id);
	sha2::Digest::update(&mut hasher, job.offer_payload);
	sha2::Digest::update(&mut hasher, job.currency);
	sha2::Digest::update(&mut hasher, job.provider_job_id);
	sha2::Digest::update(&mut hasher, job.job_created_at_sync.to_string());
	sha2::Digest::update(&mut hasher, job.job_public_payload);
	sha2::Digest::update(&mut hasher, job.job_completed_at_sync.to_string());

	if let Some(balance_delta) = &job.balance_delta {
		sha2::Digest::update(&mut hasher, balance_delta);
	}

	hasher.finalize().to_vec()
}

fn validate_balance_delta(
	balance_delta: &str,
	currency: Currency,
) -> eyre::Result<()> {
	match currency {
		Currency::Polygon => match balance_delta.parse::<U256>() {
			Ok(_) => Ok(()),
			Err(e) => Err(eyre!(e)),
		},
	}
}
