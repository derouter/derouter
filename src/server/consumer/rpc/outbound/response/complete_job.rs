use serde::Serialize;

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum CompleteJobResponse {
	Ok,

	/// Either the local P2P node is not running,
	/// or its peer ID mismatches the job's.
	InvalidConsumerPeerId {
		message: String,
	},

	InvalidJobId,
	NotSyncedYet,
	AlreadyCompleted,

	InvalidBalanceDelta {
		message: String,
	},
}
