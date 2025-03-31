use serde::Serialize;

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum CompleteJobResponse {
	Ok { completed_at_sync: i64 },
	InvalidJobId,
	AlreadyCompleted { completed_at_sync: i64 },
	AlreadyFailed,
	InvalidBalanceDelta { message: String },
}
