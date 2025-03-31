use serde::Serialize;

pub mod request;
pub mod response;

#[derive(Serialize, Debug)]
#[serde(tag = "kind")]
pub enum OutboundFrame {
	Request(request::OutboundRequestFrame),
	Response(response::OutboundResponseFrame),
}
