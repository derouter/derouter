use serde::Deserialize;

pub mod request;
pub mod response;

#[derive(Deserialize, Debug)]
#[serde(tag = "kind")]
pub enum InboundFrame {
	Request(request::InboundRequestFrame),
	Response(response::InboundResponseFrame),
}
