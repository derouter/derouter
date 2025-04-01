use serde::Serialize;

/// Ask the provider to open a service connection.
#[derive(Serialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum OutboundRequestFrameData {
	ProviderOpenConnection {
		customer_peer_id: String,
		protocol_id: String,
		offer_id: String,
		protocol_payload: serde_json::Value,
		connection_id: i64,
	},
}

#[derive(Serialize, Debug)]
pub struct OutboundRequestFrame {
	/// Incremented on our side.
	pub id: u32,

	#[serde(flatten)]
	pub data: OutboundRequestFrameData,
}
