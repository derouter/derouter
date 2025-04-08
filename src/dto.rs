use serde::Serialize;

use crate::database::service_connections::Currency;

#[derive(Serialize, Debug, Clone)]
pub struct ProviderHeartbeat {
	pub peer_id: String,

	/// The heartbeat's timestamp in our clock.
	#[serde(with = "chrono::serde::ts_seconds")]
	pub latest_heartbeat_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Debug, Clone)]
pub struct ProviderRecord {
	pub peer_id: String,

	pub name: Option<String>,
	pub teaser: Option<String>,
	pub description: Option<String>,

	/// Last time the provider was updated at, in our clock.
	#[serde(with = "chrono::serde::ts_seconds")]
	pub updated_at: chrono::DateTime<chrono::Utc>,

	/// The heartbeat's timestamp in our clock.
	#[serde(with = "chrono::serde::ts_seconds")]
	pub latest_heartbeat_at: chrono::DateTime<chrono::Utc>,
}

/// This offer has been updated with a new snapshot.
#[derive(Serialize, Debug, Clone)]
pub struct OfferSnapshot {
	pub snapshot_id: i64,
	pub active: bool,
	pub provider_peer_id: String,
	pub protocol_id: String,
	pub offer_id: String,
	pub protocol_payload: String,
}

/// This offer is not active anymore.
#[derive(Serialize, Debug, Clone)]
pub struct OfferRemoved {
	pub snapshot_id: i64,
	pub provider_peer_id: String,
	pub protocol_id: String,
	pub offer_id: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct JobRecord {
	pub job_rowid: i64,
	pub provider_job_id: Option<String>,
	pub provider_peer_id: String,
	pub consumer_peer_id: String,
	pub offer_snapshot_rowid: i64,
	pub offer_protocol_id: String,
	pub currency: Currency,
	pub balance_delta: Option<String>,
	pub public_payload: Option<String>,
	pub private_payload: Option<String>,
	pub reason: Option<String>,
	pub reason_class: Option<i64>,

	#[serde(with = "chrono::serde::ts_seconds")]
	pub created_at_local: chrono::DateTime<chrono::Utc>,

	pub created_at_sync: Option<i64>,

	#[serde(with = "chrono::serde::ts_seconds_option")]
	pub completed_at_local: Option<chrono::DateTime<chrono::Utc>>,

	pub completed_at_sync: Option<i64>,

	#[serde(with = "chrono::serde::ts_seconds_option")]
	pub signature_confirmed_at_local: Option<chrono::DateTime<chrono::Utc>>,

	pub confirmation_error: Option<String>,
}
