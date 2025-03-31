use serde::Serialize;

use crate::dto::{OfferUpdated, ProviderUpdated};

#[derive(Serialize, Debug)]
pub struct ConsumerConfigResponse {
	pub providers: Vec<ProviderUpdated>,
	pub offers: Vec<OfferUpdated>,
}
