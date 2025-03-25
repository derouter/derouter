use serde::{Deserialize, Serialize};

use crate::dto::{OfferUpdated, ProviderUpdated};

#[derive(Serialize, Deserialize, Debug)]
pub struct ConsumerConfigResponse {
	pub providers: Vec<ProviderUpdated>,
	pub offers: Vec<OfferUpdated>,
}
