use consumer::{
	complete_job::{ConsumerCompleteJobRequest, ConsumerCompleteJobResponse},
	create_job::{ConsumerCreateJobRequest, ConsumerCreateJobResponse},
	get_job::{ConsumerGetJobRequest, ConsumerGetJobResponse},
	open_job_connection::{
		ConsumerOpenJobConnectionRequest, ConsumerOpenJobConnectionResponse,
	},
};
use fail_job::{FailJobRequest, FailJobResponse};
use provider::{
	complete_job::{ProviderCompleteJobRequest, ProviderCompleteJobResponse},
	create_job::ProviderCreateJobResponse,
	job_connection::ProviderPrepareJobConnectionResponse,
	provide::{ProviderProvideRequest, ProviderProvideResponse},
};
use query::{
	active_offers::{ActiveOffersQueryRequest, ActiveOffersQueryResponse},
	active_providers::{
		ActiveProvidersQueryRequest, ActiveProvidersQueryResponse,
	},
	jobs::{JobsQueryRequest, JobsQueryResponse},
	offer_snapshots::{OfferSnapshotsQueryRequest, OfferSnapshotsQueryResponse},
	providers::{ProvidersQueryRequest, ProvidersQueryResponse},
	system::{SystemQueryRequest, SystemQueryResponse},
};
use serde::{Deserialize, Serialize};
use subscription::{
	active_offers::{
		ActiveOffersSubscriptionRequest, ActiveOffersSubscriptionResponse,
	},
	active_providers::{
		ActiveProvidersSubscriptionRequest, ActiveProvidersSubscriptionResponse,
	},
	cancel::{CancelSubscriptionRequest, CancelSubscriptionResponse},
	jobs::{JobsSubscriptionRequest, JobsSubscriptionResponse},
};

use crate::dto::{
	JobRecord, OfferRemoved, OfferSnapshot, ProviderHeartbeat, ProviderRecord,
};

pub mod consumer;
pub mod fail_job;
pub mod provider;
pub mod query;
pub mod subscription;

#[derive(Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum InboundRequestFrameData {
	FailJob(FailJobRequest),

	ConsumerCompleteJob(ConsumerCompleteJobRequest),
	ConsumerCreateJob(ConsumerCreateJobRequest),
	ConsumerGetJob(ConsumerGetJobRequest),
	ConsumerOpenJobConnection(ConsumerOpenJobConnectionRequest),

	ProviderCompleteJob(ProviderCompleteJobRequest),
	ProviderProvide(ProviderProvideRequest),

	QueryJobs(JobsQueryRequest),
	QueryOfferSnapshots(OfferSnapshotsQueryRequest),
	QueryActiveProviders(ActiveProvidersQueryRequest),
	QueryActiveOffers(ActiveOffersQueryRequest),
	QueryProviders(ProvidersQueryRequest),
	QuerySystem(SystemQueryRequest),

	CancelSubscription(CancelSubscriptionRequest),
	SubscribeToJobs(JobsSubscriptionRequest),
	SubscribeToActiveOffers(ActiveOffersSubscriptionRequest),
	SubscribeToActiveProviders(ActiveProvidersSubscriptionRequest),
}

#[derive(Deserialize, Debug)]
pub struct InboundRequestFrame {
	/// Incremented on their side.
	pub id: u32,

	#[serde(flatten)]
	pub data: InboundRequestFrameData,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum InboundResponseFrameData {
	Ack,
	ProviderCreateJob(ProviderCreateJobResponse),
	ProviderPrepareJobConnection(ProviderPrepareJobConnectionResponse),
}

#[derive(Deserialize, Debug)]
pub struct InboundResponseFrame {
	/// The `OutboundRequestFrame.id` this response is for.
	pub id: u32,

	#[serde(flatten)]
	pub data: InboundResponseFrameData,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "kind")]
pub enum InboundFrame {
	Request(InboundRequestFrame),
	Response(InboundResponseFrame),
}

#[derive(Serialize, derive_more::Debug)]
#[serde(tag = "type", content = "data")]
pub enum OutboundRequestFrameData {
	EventOfferRemoved {
		subscription_id: u32,

		#[debug(skip)]
		payload: OfferRemoved,
	},

	EventOfferUpdated {
		subscription_id: u32,

		#[debug(skip)]
		payload: OfferSnapshot,
	},

	EventProviderHeartbeat {
		subscription_id: u32,

		#[debug(skip)]
		payload: ProviderHeartbeat,
	},

	EventProviderUpdated {
		subscription_id: u32,

		#[debug(skip)]
		payload: ProviderRecord,
	},

	EventJobUpdated {
		subscription_id: u32,

		#[debug(skip)]
		payload: Box<JobRecord>,
	},

	/// Sent to a Provider module when Consumer asks for a new job.
	ProviderCreateJob {
		provider_peer_id: libp2p::PeerId,
		protocol_id: String,
		offer_id: String,
		provider_job_id: String,
		created_at_sync: i64,

		/// Optional job arguments for immediate processing, if defined by protocol.
		#[debug(skip)]
		job_args: Option<String>,
	},

	/// Sent to a Provider module when Consumer
	/// wants to open a job connection.
	ProviderPrepareJobConnection {
		provider_peer_id: libp2p::PeerId,
		provider_job_id: String,
	},

	/// Sent after [ProviderPrepareJobConnection] is verified.
	ProviderOpenJobConnection { connection_id: u64, nonce: String },
}

#[derive(Serialize, Debug)]
pub struct OutboundRequestFrame {
	/// An ever-incrementing (per RPC connection) request counter.
	pub id: u32,

	#[serde(flatten)]
	pub data: OutboundRequestFrameData,
}

#[derive(Serialize, Debug)]
#[serde(tag = "type", content = "data")]
#[allow(clippy::enum_variant_names)]
pub enum OutboundResponseFrameData {
	FailJob(FailJobResponse),

	ConsumerCompleteJob(ConsumerCompleteJobResponse),
	ConsumerCreateJob(ConsumerCreateJobResponse),
	ConsumerGetJob(ConsumerGetJobResponse),
	ConsumerOpenJobConnection(ConsumerOpenJobConnectionResponse),

	ProviderCompleteJob(ProviderCompleteJobResponse),
	ProviderProvide(ProviderProvideResponse),

	QueryJobs(JobsQueryResponse),
	QueryOfferSnapshots(OfferSnapshotsQueryResponse),
	QueryActiveProviders(ActiveProvidersQueryResponse),
	QueryActiveOffers(ActiveOffersQueryResponse),
	QueryProviders(ProvidersQueryResponse),
	QuerySystem(SystemQueryResponse),

	CancelSubscription(CancelSubscriptionResponse),
	SubscribeToJobs(JobsSubscriptionResponse),
	SubscribeToActiveOffers(ActiveOffersSubscriptionResponse),
	SubscribeToActiveProviders(ActiveProvidersSubscriptionResponse),
}

#[derive(Serialize, Debug)]
pub struct OutboundResponseFrame {
	/// The `InboundRequestFrame.id` this response is for.
	pub id: u32,

	#[serde(flatten)]
	pub data: OutboundResponseFrameData,
}

#[derive(Serialize, Debug)]
#[serde(tag = "kind")]
pub enum OutboundFrame {
	Request(OutboundRequestFrame),
	Response(OutboundResponseFrame),
}
