use serde::{Deserialize, Serialize};

use crate::{
	db::{self, service_jobs::create::create_job},
	dto::Currency,
	p2p,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
	util::format_secret_option,
};

/// Send a CreateJob request to the Provider. If succeeded, Provider may
/// or may not immediately start processing the job in background
/// (defined by the protocol). After the job is created, the Consumer
/// should open a new connection to the job to get its result,
/// or to feed it with arguments.
#[derive(Deserialize, derive_more::Debug)]
pub struct ConsumerCreateJobRequest {
	/// Local offer snapshot ID.
	offer_snapshot_id: i64,

	/// Currency enum.
	currency: Currency,

	/// Optional job arguments for immediate processing, if defined by protocol.
	#[debug("{}", format_secret_option(job_args))]
	job_args: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ConsumerCreateJobResponse {
	Ok {
		provider_peer_id: libp2p::PeerId,
		provider_job_id: String,
	},

	/// Offer snapshot not found by `offer_snapshot_id`.
	LocalOfferNotFound,

	/// Provider peer is unreacheable via P2P.
	ProviderUnreacheable,

	/// Provider's response violates the protocol.
	ProviderInvalidResponse,

	/// Provider claims that they don't have this offer.
	/// The offer snapshot may be stale.
	ProviderOfferNotFound,

	/// Provider claims offer payload mismatch.
	/// The offer snapshot may be stale.
	ProviderOfferPayloadMismatch,

	/// Provider claims `job_args` to be invalid.
	InvalidJobArgs(String),

	/// Provider is temporarily busy for this offer.
	ProviderBusy,
}

impl Connection {
	pub async fn handle_consumer_create_job_request(
		&self,
		request_id: u32,
		request: ConsumerCreateJobRequest,
	) {
		let response = 'block: {
			let snapshot = match db::offers::find(
				&*self.state.db.lock().await,
				request.offer_snapshot_id,
			) {
				Some(x) => x,
				None => {
					break 'block Some(ConsumerCreateJobResponse::LocalOfferNotFound);
				}
			};

			if !snapshot.active {
				log::warn!("Offer snapshot is inactive: {}", request.offer_snapshot_id);
				break 'block Some(ConsumerCreateJobResponse::LocalOfferNotFound);
			}

			// We'll now send a create job request to Provider via P2P network...
			//

			let p2p = self.state.p2p.lock().await;
			let reqres_request_tx = p2p.reqres_request_tx.clone();
			let consumer_peer_id = p2p.keypair.public().to_peer_id();
			drop(p2p);

			let (response_tx, response_rx) = tokio::sync::oneshot::channel();

			reqres_request_tx
				.send(p2p::reqres::OutboundRequestEnvelope {
					request: p2p::proto::request_response::Request::CreateJob(
						p2p::proto::request_response::CreateJobRequest {
							protocol_id: snapshot.protocol_id,
							offer_id: snapshot.offer_id,
							offer_payload: snapshot.protocol_payload,
							job_args: request.job_args.clone(),
							currency: request.currency,
						},
					),
					response_tx,
					target_peer_id: snapshot.provider_peer_id,
				})
				.await
				.unwrap();

			let db = self.state.db.clone();
			let outbound_tx = self.outbound_tx.clone();

			// ...and wait for the response in a separate task.
			//

			tokio::spawn(async move {
				let rpc_response = {
					type Response = p2p::proto::request_response::Response;

					match response_rx.await.unwrap() {
						Ok(response) => match response {
							Response::CreateJob(provider_create_job_response) => {
								type Response = p2p::proto::request_response::CreateJobResponse;

								match provider_create_job_response {
									Response::Ok {
										provider_job_id: job_id,
										created_at_sync,
									} => {
										create_job(
											&mut *db.lock().await,
											request.offer_snapshot_id,
											consumer_peer_id,
											request.currency,
											request.job_args.as_deref(),
											&job_id,
											created_at_sync,
										);

										// Ok! 🎉
										ConsumerCreateJobResponse::Ok {
											provider_job_id: job_id,
											provider_peer_id: snapshot.provider_peer_id,
										}
									}

									Response::OfferNotFound => {
										ConsumerCreateJobResponse::ProviderOfferNotFound
									}

									Response::OfferPayloadMismatch => {
										ConsumerCreateJobResponse::ProviderOfferPayloadMismatch
									}

									Response::InvalidJobArgs(err) => {
										ConsumerCreateJobResponse::InvalidJobArgs(err)
									}

									Response::Busy => ConsumerCreateJobResponse::ProviderBusy,
								}
							}

							_ => {
								log::warn!("Unexpected response from provider");
								ConsumerCreateJobResponse::ProviderInvalidResponse
							}
						},

						Err(e) => {
							log::warn!("{e:?}");
							ConsumerCreateJobResponse::ProviderUnreacheable
						}
					}
				};

				let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
					id: request_id,
					data: OutboundResponseFrameData::ConsumerCreateJob(rpc_response),
				});

				let _ = outbound_tx.send(outbound_frame).await;
			});

			None
		};

		if let Some(response) = response {
			let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
				id: request_id,
				data: OutboundResponseFrameData::ConsumerCreateJob(response),
			});

			let _ = self.outbound_tx.send(outbound_frame).await;
		}
	}
}
