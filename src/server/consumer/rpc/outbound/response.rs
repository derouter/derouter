use complete_job::CompleteJobResponse;
use config::ConsumerConfigResponse;
use confirm_job_completion::ConfirmJobCompletionResponse;
use create_job::CreateJobResponse;
use fail_job::FailJobResponse;
use open_connection::OpenConnectionResponse;
use serde::Serialize;
use sync_job::SyncJobResponse;

pub mod complete_job;
pub mod config;
pub mod confirm_job_completion;
pub mod create_job;
pub mod fail_job;
pub mod open_connection;
pub mod sync_job;

#[derive(Serialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum OutboundResponseFrameData {
	Config(ConsumerConfigResponse),
	OpenConnection(OpenConnectionResponse),
	CreateJob(CreateJobResponse),
	CompleteJob(CompleteJobResponse),
	ConfirmJobCompletion(ConfirmJobCompletionResponse),
	FailJob(FailJobResponse),
	SyncJob(SyncJobResponse),
}

#[derive(Serialize, Debug)]
pub struct OutboundResponseFrame {
	/// The `InboundRequestFrame.id` this response is for.
	pub id: u32,

	#[serde(flatten)]
	pub data: OutboundResponseFrameData,
}
