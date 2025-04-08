use alloy_primitives::U256;
use sha2::Digest;

use crate::db::service_connections::Currency;

pub mod consumer;
pub mod fail;
pub mod get;
pub mod provider;
pub mod query_all;
pub mod set_confirmation_error;

// OPTIMIZE: Make it borrow values instead of owning.
struct JobHashingPayload {
	consumer_peer_id: String,
	provider_peer_id: String,
	protocol_id: String,
	offer_payload: String,
	currency: String,
	provider_job_id: String,
	job_created_at_sync: i64,
	job_public_payload: String,
	job_completed_at_sync: i64,
	balance_delta: Option<String>,
}

fn hash_job(job: &JobHashingPayload) -> Vec<u8> {
	let mut hasher = sha2::Sha256::new();

	sha2::Digest::update(&mut hasher, &job.consumer_peer_id);
	sha2::Digest::update(&mut hasher, &job.provider_peer_id);
	sha2::Digest::update(&mut hasher, &job.protocol_id);
	sha2::Digest::update(&mut hasher, &job.offer_payload);
	sha2::Digest::update(&mut hasher, &job.currency);
	sha2::Digest::update(&mut hasher, &job.provider_job_id);
	sha2::Digest::update(&mut hasher, job.job_created_at_sync.to_string());
	sha2::Digest::update(&mut hasher, &job.job_public_payload);
	sha2::Digest::update(&mut hasher, job.job_completed_at_sync.to_string());

	if let Some(balance_delta) = &job.balance_delta {
		sha2::Digest::update(&mut hasher, balance_delta);
	}

	hasher.finalize().to_vec()
}

fn validate_balance_delta(
	balance_delta: &str,
	currency: Currency,
) -> Option<String> {
	match currency {
		Currency::Polygon => match balance_delta.parse::<U256>() {
			Ok(_) => None,
			Err(e) => Some(format!("{}", e)),
		},
	}
}
