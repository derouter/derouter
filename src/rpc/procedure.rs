use consumer::{
	complete_job::{ConsumerCompleteJobRequest, ConsumerCompleteJobResponse},
	confirm_job_completion::{
		ConsumerConfirmJobCompletionRequest, ConsumerConfirmJobCompletionResponse,
	},
	create_job::{ConsumerCreateJobRequest, ConsumerCreateJobResponse},
	fail_job::{ConsumerFailJobRequest, ConsumerFailJobResponse},
	open_connection::{
		ConsumerOpenConnectionRequest, ConsumerOpenConnectionResponse,
	},
	sync_job::{ConsumerSyncJobRequest, ConsumerSyncJobResponse},
};
use provider::{
	complete_job::{ProviderCompleteJobRequest, ProviderCompleteJobResponse},
	create_job::{ProviderCreateJobRequest, ProviderCreateJobResponse},
	fail_job::{ProviderFailJobRequest, ProviderFailJobResponse},
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
pub mod provider;
pub mod query;
pub mod subscription;

#[derive(Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum InboundRequestFrameData {
	ConsumerCompleteJob(ConsumerCompleteJobRequest),
	ConsumerConfirmJobCompletion(ConsumerConfirmJobCompletionRequest),
	ConsumerCreateJob(ConsumerCreateJobRequest),
	ConsumerFailJob(ConsumerFailJobRequest),
	ConsumerOpenConnection(ConsumerOpenConnectionRequest),
	ConsumerSyncJob(ConsumerSyncJobRequest),

	ProviderCompleteJob(ProviderCompleteJobRequest),
	ProviderCreateJob(ProviderCreateJobRequest),
	ProviderFailJob(ProviderFailJobRequest),
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

#[derive(Serialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum OutboundRequestFrameData {
	EventOfferRemoved {
		subscription_id: u32,
		payload: OfferRemoved,
	},

	EventOfferUpdated {
		subscription_id: u32,
		payload: OfferSnapshot,
	},

	EventProviderHeartbeat {
		subscription_id: u32,
		payload: ProviderHeartbeat,
	},

	EventProviderUpdated {
		subscription_id: u32,
		payload: ProviderRecord,
	},

	EventJobUpdated {
		subscription_id: u32,
		payload: Box<JobRecord>,
	},

	ProviderOpenConnection {
		customer_peer_id: String,
		protocol_id: String,
		offer_id: String,
		protocol_payload: serde_json::Value,
		connection_id: i64,
	},
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
	ConsumerCompleteJob(ConsumerCompleteJobResponse),
	ConsumerConfirmJobCompletion(ConsumerConfirmJobCompletionResponse),
	ConsumerCreateJob(ConsumerCreateJobResponse),
	ConsumerFailJob(ConsumerFailJobResponse),
	ConsumerOpenConnection(ConsumerOpenConnectionResponse),
	ConsumerSyncJob(ConsumerSyncJobResponse),

	ProviderCompleteJob(ProviderCompleteJobResponse),
	ProviderCreateJob(ProviderCreateJobResponse),
	ProviderFailJob(ProviderFailJobResponse),
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
