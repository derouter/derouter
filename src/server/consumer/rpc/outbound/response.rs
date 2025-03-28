use config::ConsumerConfigResponse;
use open_connection::OpenConnectionResponse;
use serde::{Deserialize, Serialize};

pub mod config;
pub mod open_connection;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum OutboundResponseFrameData {
	Ack,
	Config(ConsumerConfigResponse),
	OpenConnection(OpenConnectionResponse),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OutboundResponseFrame {
	/// The `InboundRequestFrame.id` this response is for.
	pub id: u32,

	#[serde(flatten)]
	pub data: OutboundResponseFrameData,
}
