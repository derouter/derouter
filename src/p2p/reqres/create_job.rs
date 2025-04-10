use libp2p::{PeerId, request_response::ResponseChannel};
use uuid::Uuid;

use crate::{
	db::service_jobs::create::create_job,
	p2p::proto::{
		self,
		request_response::{CreateJobRequest, CreateJobResponse, Response},
	},
	rpc,
};

use crate::p2p::Node;

impl Node {
	pub(super) async fn handle_create_job_request(
		&mut self,
		from_peer_id: PeerId,
		request: CreateJobRequest,
		response_channel: ResponseChannel<proto::request_response::Response>,
	) {
		let provider_lock = self.state.provider.lock().await;

		let offer =
			provider_lock.find_offer(&request.protocol_id, &request.offer_id);

		let response = if let Some(offer) = offer {
			if offer.provided.protocol_payload == *request.offer_payload {
				let provider_job_id = Uuid::new_v4().to_string();
				let created_at_sync = chrono::Utc::now().timestamp();

				let (envelope, response_rx) = rpc::OutboundRequestEnvelope::new(
					rpc::OutboundRequestFrameData::ProviderCreateJob {
						provider_peer_id: *self.swarm.local_peer_id(),
						protocol_id: request.protocol_id,
						offer_id: request.offer_id,
						provider_job_id: provider_job_id.clone(),
						created_at_sync,
						job_args: request.job_args.clone(),
					},
				);

				offer
					.module
					.outbound_request_tx
					.send(envelope)
					.await
					.unwrap();

				match response_rx.await.unwrap() {
					rpc::InboundResponseFrameData::ProviderCreateJob(response) => {
						type RpcResponse =
							rpc::procedure::provider::create_job::ProviderCreateJobResponse;

						match response {
							RpcResponse::Ok => {
								create_job(
									&mut *self.state.db.lock().await,
									offer.provided.snapshot_rowid,
									from_peer_id,
									request.currency,
									request.job_args.as_deref(),
									&provider_job_id,
									created_at_sync,
								);

								CreateJobResponse::Ok {
									provider_job_id,
									created_at_sync,
								}
							}

							RpcResponse::Busy => CreateJobResponse::Busy,

							RpcResponse::InvalidJobArgs(err) => {
								CreateJobResponse::InvalidJobArgs(err)
							}
						}
					}

					r => {
						log::error!("Unexpected response from provider module: {r:?}");
						CreateJobResponse::Busy
					}
				}
			} else {
				log::debug!(
					"Protocol payload mismatch (our: {}, their: {})",
					offer.provided.protocol_payload,
					request.offer_payload
				);

				CreateJobResponse::OfferPayloadMismatch
			}
		} else {
			log::debug!(
				r#"Could not find offer "{}" => "{}""#,
				request.protocol_id,
				request.offer_id
			);

			CreateJobResponse::OfferNotFound
		};

		let response = Response::CreateJob(response);
		log::debug!("Sending {:?}..", response);

		// TODO: Wait for outbound failures.
		match self
			.swarm
			.behaviour_mut()
			.request_response
			.send_response(response_channel, response)
		{
			Ok(_) => {}
			Err(response) => {
				log::warn!("Failed to send {:?}", response)
			}
		}
	}
}
