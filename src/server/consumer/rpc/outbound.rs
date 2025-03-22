use serde::{Deserialize, Serialize};

pub mod response;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "kind")]
pub enum OutboundFrame {
	Response(response::OutboundResponseFrame),
}
