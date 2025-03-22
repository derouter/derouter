use serde::{Deserialize, Serialize};

pub mod config;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum InboundRequestFrameData {
	Config(config::ProviderConfig),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct InboundRequestFrame {
	/// Incremented on their side.
	pub id: u32,

	#[serde(flatten)]
	pub data: InboundRequestFrameData,
}
