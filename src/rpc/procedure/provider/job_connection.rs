use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ProviderPrepareJobConnectionResponse {
	/// Includes the module-defined connection nonce.
	Ok(String),

	JobNotFound,
	Busy,
}
