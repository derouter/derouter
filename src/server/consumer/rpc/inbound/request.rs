use config::ConsumerConfig;
use serde::Deserialize;

use crate::database::service_connections::Currency;

pub mod config;

#[derive(Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum InboundRequestFrameData {
	Config(ConsumerConfig),

	/// Open a new connection for given offer.
	OpenConnection {
		/// Local offer snapshot ID.
		offer_snapshot_id: i64,

		/// Currency enum.
		currency: Currency,
	},

	/// Create a new "uninitialized" service job locally,
	/// independent from the Provider. An uninitialized job
	/// may be used to record pre-response Provider errors,
	/// when its `provider_job_id` is not yet known.
	/// Shall call [SyncJob] before [CompleteJob].
	CreateJob {
		/// The associated connection ID.
		connection_id: i64,

		/// Private job payload, stored locally.
		private_payload: Option<String>,
	},

	/// Synchronize a previosuly [created](CreateJob) job with Provider's data.
	/// Useful for long-running jobs, **mandatory** before a [CompleteJob] call.
	SyncJob {
		/// Job ID returned by [`CreateJob`].
		database_job_id: i64,

		/// Job ID as told by the Provider.
		provider_job_id: String,

		/// Optionally update the local private payload.
		private_payload: Option<String>,

		/// Job creation timestamp as told by the Provider, in Provider's clock.
		created_at_sync: i64,
	},

	/// Mark a previously [synchronized](SyncJob) job as completed.
	/// Shall call [ConfirmJobCompletion] afterwards.
	CompleteJob {
		/// Job ID returned by [`CreateJob`].
		database_job_id: i64,

		/// Publicly-available job completion timestamp, told by Provider.
		completed_at_sync: i64,

		/// Publicly-available balance delta in [Currency]-specific encoding.
		balance_delta: Option<String>,

		/// Publicly-available job payload.
		public_payload: String,

		/// Private, local-stored payload.
		/// Would override if already set.
		private_payload: Option<String>,
	},

	/// Confirm a job completion signature with the Provider.
	/// Shall be called after [CompleteJob].
	ConfirmJobCompletion {
		/// Job ID returned by [`CreateJob`].
		database_job_id: i64,
	},

	/// Mark a previously [created](CreateJob) job as failed.
	/// May also be called after [SyncJob].
	FailJob {
		/// Job ID returned by [`CreateJob`].
		database_job_id: i64,

		/// The failure reason.
		reason: String,

		/// Protocol-specific faiulre class.
		reason_class: Option<i64>,

		/// Private, local-stored payload.
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
