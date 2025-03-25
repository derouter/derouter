use serde::{Deserialize, Serialize};

use crate::dto::{
	OfferRemoved, OfferUpdated, ProviderHeartbeat, ProviderUpdated,
};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum OutboundRequestFrameData {
	OfferRemoved(OfferRemoved),
	OfferUpdated(OfferUpdated),
	ProviderHeartbeat(ProviderHeartbeat),
	ProviderUpdated(ProviderUpdated),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OutboundRequestFrame {
	/// An ever-incrementing request counter.
	pub id: u32,

	#[serde(flatten)]
	pub data: OutboundRequestFrameData,
}
