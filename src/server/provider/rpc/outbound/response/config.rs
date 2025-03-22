use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ConfigResponse {
	Ok,
	AlreadyConfigured,
	DuplicateOffer {
		protocol_id: String,
		offer_id: String,
	},
}
