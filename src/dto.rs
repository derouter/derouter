use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProviderHeartbeat {
	pub peer_id: libp2p::PeerId,

	/// The heartbeat's timestamp in our clock.
	#[serde(with = "chrono::serde::ts_seconds")]
	pub last_heartbeat_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProviderUpdated {
	pub peer_id: libp2p::PeerId,

	pub name: Option<String>,
	pub teaser: Option<String>,
	pub description: Option<String>,

	/// Last time the provider was updated at, in our clock.
	#[serde(with = "chrono::serde::ts_seconds")]
	pub updated_at: chrono::DateTime<chrono::Utc>,

	/// The heartbeat's timestamp in our clock.
	#[serde(with = "chrono::serde::ts_seconds")]
	pub last_heartbeat_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OfferUpdated {
	pub provider_peer_id: libp2p::PeerId,
	pub protocol_id: String,
	pub offer_id: String,
	pub _protocol_payload: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OfferRemoved {
	pub _provider_peer_id: libp2p::PeerId,
	pub protocol_id: String,
	pub _offer_id: String,
}
