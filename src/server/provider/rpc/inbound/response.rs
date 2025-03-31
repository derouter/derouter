use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum InboundResponseFrameData {
	Ack,
}

#[derive(Deserialize, Debug)]
pub struct InboundResponseFrame {
	/// The `OutboundRequestFrame.id` this response is for.
	pub id: u32,

	#[serde(flatten)]
	pub data: InboundResponseFrameData,
}
