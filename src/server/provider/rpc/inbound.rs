use serde::{Deserialize, Serialize};

pub mod request;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "kind")]
pub enum InboundFrame {
	Request(request::InboundRequestFrame),
}
