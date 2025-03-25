use config::ConsumerConfigResponse;
use serde::{Deserialize, Serialize};

pub mod config;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum OutboundResponseFrameData {
	Ack,
	Config(ConsumerConfigResponse),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OutboundResponseFrame {
	/// The `InboundRequestFrame.id` this response is for.
	pub id: u32,

	#[serde(flatten)]
	pub data: OutboundResponseFrameData,
}
