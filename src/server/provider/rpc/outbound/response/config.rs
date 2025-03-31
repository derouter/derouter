use serde::Serialize;

#[derive(Serialize, Debug)]
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
