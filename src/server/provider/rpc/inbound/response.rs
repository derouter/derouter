use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum InboundResponseFrameData {
	Ack,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct InboundResponseFrame {
	/// The `OutboundRequestFrame.id` this response is for.
	pub id: u32,

	#[serde(flatten)]
	pub data: InboundResponseFrameData,
}
