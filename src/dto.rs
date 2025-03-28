use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProviderHeartbeat {
	pub peer_id: String,

	/// The heartbeat's timestamp in our clock.
	#[serde(with = "chrono::serde::ts_seconds")]
	pub latest_heartbeat_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProviderUpdated {
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
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OfferUpdated {
	/// Offer snapshot ID, unique per database.
	pub snapshot_id: i64,

	pub provider_peer_id: String,
	pub protocol_id: String,
	pub offer_id: String,
	pub protocol_payload: serde_json::Value,
}

/// This offer is not available anymore.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OfferRemoved {
	pub snapshot_id: i64,
}
