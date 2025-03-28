use serde::{Deserialize, Serialize};

pub mod request;
pub mod response;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "kind")]
pub enum OutboundFrame {
	Request(request::OutboundRequestFrame),
	Response(response::OutboundResponseFrame),
}
