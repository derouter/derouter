use complete_job::CompleteJobResponse;
use config::ConfigResponse;
use create_job::CreateJobResponse;
use fail_job::FailJobResponse;
use serde::Serialize;

pub mod complete_job;
pub mod config;
pub mod create_job;
pub mod fail_job;

#[derive(Serialize, Debug)]
#[serde(tag = "type", content = "data")]
#[allow(clippy::enum_variant_names)]
pub enum OutboundResponseFrameData {
	ProviderConfig(ConfigResponse),
	ProviderCreateJob(CreateJobResponse),
	ProviderCompleteJob(CompleteJobResponse),
	ProviderFailJob(FailJobResponse),
}

#[derive(Serialize, Debug)]
pub struct OutboundResponseFrame {
	/// The `InboundRequestFrame.id` this response is for.
	pub id: u32,

	#[serde(flatten)]
	pub data: OutboundResponseFrameData,
}
