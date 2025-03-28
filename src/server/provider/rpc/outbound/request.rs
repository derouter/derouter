use serde::{Deserialize, Serialize};

/// Ask the provider to open a service connection.
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum OutboundRequestFrameData {
	OpenConnection {
		customer_peer_id: String,
		protocol_id: String,
		offer_id: String,
		protocol_payload: serde_json::Value,
		connection_id: i64,
	},
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OutboundRequestFrame {
	/// Incremented on our side.
	pub id: u32,

	#[serde(flatten)]
	pub data: OutboundRequestFrameData,
}
