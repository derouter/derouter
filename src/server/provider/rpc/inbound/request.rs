use serde::Deserialize;

pub mod config;

#[derive(Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
#[allow(clippy::enum_variant_names)]
pub enum InboundRequestFrameData {
	ProviderConfig(config::ProviderConfig),

	/// Create a new incomplete service job
	/// associated with given connection, locally.
	ProviderCreateJob {
		/// Local connection ID.
		connection_id: i64,

		/// Private job payload, stored locally.
		private_payload: Option<String>,
	},

	/// Mark a job as completed, locally.
	ProviderCompleteJob {
		/// Local job ID.
		database_job_id: i64,

		/// Balance delta in currency-specific formatting.
		balance_delta: Option<String>,

		/// Potentially-publicly-accessible job payload,
		/// to be signed by the Consumer.
		public_payload: String,

		/// Private job payload, stored locally.
		/// Would override if already set.
		private_payload: Option<String>,
	},

	/// Mark a job as failed, locally.
	ProviderFailJob {
		/// Local job ID.
		database_job_id: i64,

		/// Reason for the failure.
		reason: String,

		/// Protocol-specific failure class, if any.
		reason_class: Option<i64>,

		/// Private job payload, stored locally.
		/// Would override if already set.
		private_payload: Option<String>,
	},
}

#[derive(Deserialize, Debug)]
pub struct InboundRequestFrame {
	/// Incremented on their side.
	pub id: u32,

	#[serde(flatten)]
	pub data: InboundRequestFrameData,
}
