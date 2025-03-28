use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum OpenConnectionResponse {
	Ok {
		/// Local service connection ID.
		connection_id: i64,
	},

	ProviderUnreacheableError,
	ProviderOfferNotFoundError,

	OtherRemoteError(String),
	OtherLocalError(String),
}
