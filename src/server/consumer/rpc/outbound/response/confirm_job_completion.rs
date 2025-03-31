use serde::Serialize;

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ConfirmJobCompletionResponse {
	Ok,

	/// Either the local P2P node is not running,
	/// or its peer ID mismatches the job's.
	InvalidConsumerPeerId {
		message: String,
	},

	InvalidJobId,
	NotCompletedYet,
	AlreadyFailed,

	/// The job's signature  is already confirmed (not a error).
	AlreadyConfirmed,

	ProviderUnreacheable,
	ProviderError(String),
}
