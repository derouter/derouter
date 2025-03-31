use serde::Serialize;

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum FailJobResponse {
	Ok,
	InvalidJobId,
	AlreadyCompleted,
	AlreadyFailed,
}
