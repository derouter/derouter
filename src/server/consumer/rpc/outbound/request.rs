use serde::Serialize;

use crate::dto::{
	OfferRemoved, OfferUpdated, ProviderHeartbeat, ProviderUpdated,
};

#[derive(Serialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum OutboundRequestFrameData {
	OfferRemoved(OfferRemoved),
	OfferUpdated(OfferUpdated),
	ProviderHeartbeat(ProviderHeartbeat),
	ProviderUpdated(ProviderUpdated),
}

#[derive(Serialize, Debug)]
pub struct OutboundRequestFrame {
	/// An ever-incrementing request counter.
	pub id: u32,

	#[serde(flatten)]
	pub data: OutboundRequestFrameData,
}
