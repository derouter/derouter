use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct ProviderOffer {
	pub protocol: String,
	pub protocol_payload: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ProviderConfig {
	pub provider_id: String,
	pub offers: HashMap<String, ProviderOffer>,
}
