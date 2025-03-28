use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ConfigResponse {
	Ok,
	AlreadyConfigured,
	IdAlreadyUsed,
	DuplicateOffer {
		protocol_id: String,
		offer_id: String,
	},
}
