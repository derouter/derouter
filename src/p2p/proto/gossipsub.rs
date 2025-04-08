use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProviderOffer {
	pub protocol_payload: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ProviderDetails {
	pub name: Option<String>,
	pub teaser: Option<String>,
	pub description: Option<String>,

	/// `{ ProtocolId => { OfferId => Offer } }`.
	pub offers: HashMap<String, HashMap<String, ProviderOffer>>,

	/// When was the provider last updated at, in peer's clock.
	#[serde(with = "chrono::serde::ts_seconds")]
	pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Heartbeat {
	pub provider: Option<ProviderDetails>,

	/// The heartbeat timestamp in peer's clock.
	#[serde(with = "chrono::serde::ts_seconds")]
	pub timestamp: chrono::DateTime<chrono::Utc>,
}
