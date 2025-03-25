use config::ConsumerConfig;
use open_connection::OpenConnection;
use serde::{Deserialize, Serialize};

pub mod config;
pub mod open_connection;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum InboundRequestFrameData {
	Config(ConsumerConfig),
	OpenConnection(OpenConnection),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct InboundRequestFrame {
	/// Incremented on their side.
	pub id: u32,

	#[serde(flatten)]
	pub data: InboundRequestFrameData,
}
