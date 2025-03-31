use serde::Deserialize;

pub mod request;

#[derive(Deserialize, Debug)]
#[serde(tag = "kind")]
pub enum InboundFrame {
	Request(request::InboundRequestFrame),
}
