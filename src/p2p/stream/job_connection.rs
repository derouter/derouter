use std::sync::Arc;

use tokio_util::compat::Compat;

use crate::{
	db,
	p2p::{
		Node,
		proto::stream::{
			HeadResponse, JobConnectionHeadRequest, JobConnectionHeadResponse,
		},
	},
	rpc::{self},
	state::SharedState,
	util::cbor::write_cbor,
};

impl Node {
	pub(super) async fn handle_job_connection_stream(
		state: Arc<SharedState>,
		from_peer_id: libp2p::PeerId,
		to_peer_id: libp2p::PeerId,
		request: JobConnectionHeadRequest,
		mut stream: Compat<libp2p::Stream>,
	) {
		let provider_lock = state.provider.lock().await;

		let (response, payload) = 'a: {
			let job = match db::service_jobs::get::find_by_job_id(
				db::ConnectionLike::Connection(&*state.db.lock().await),
				to_peer_id,
				&request.provider_job_id,
			) {
				Some(job) => job,
				None => break 'a (JobConnectionHeadResponse::JobNotFound, None),
			};

			if from_peer_id != job.consumer_peer_id {
				break 'a (JobConnectionHeadResponse::JobNotFound, None);
			}

			let offer =
				match provider_lock.find_offer(&job.offer_protocol_id, &job.offer_id) {
					Some(offer) => offer,
					None => {
						log::warn!(
							r#"Active offer "{}"/"{}" not found for job #{}"#,
							job.offer_protocol_id,
							job.offer_id,
							job.job_rowid
						);

						break 'a (JobConnectionHeadResponse::JobExpired, None);
					}
				};

			// We'll send `ProviderPrepareJobConnection` to make sure
			// that module is ready to accept the connection.
			//

			let (envelope, response_rx) = rpc::OutboundRequestEnvelope::new(
				rpc::OutboundRequestFrameData::ProviderPrepareJobConnection {
					provider_peer_id: to_peer_id,
					provider_job_id: request.provider_job_id,
				},
			);

			offer
				.module
				.outbound_request_tx
				.try_send(envelope)
				.expect("module's outbound_request_tx should be healthy");

			match response_rx.await {
				Ok(frame_data) => match frame_data {
					rpc::InboundResponseFrameData::ProviderPrepareJobConnection(
						response,
					) => {
						type R = rpc::procedure::provider::job_connection::ProviderPrepareJobConnectionResponse;

						match response {
							R::Ok(nonce) => {
								(JobConnectionHeadResponse::Ok, Some((offer.module, nonce)))
							}
							R::JobNotFound => (JobConnectionHeadResponse::JobNotFound, None),
							R::Busy => (JobConnectionHeadResponse::Busy, None),
						}
					}

					unexpected => {
						log::warn!(
							"Unexpected provider module's response \
								to ProviderPrepareJobConnection: {unexpected:?}"
						);

						(JobConnectionHeadResponse::Busy, None)
					}
				},

				Err(e) => {
					log::warn!(
						"While waiting for provider module's \
							response to ProviderPrepareJobConnection: {e:?}"
					);

					(JobConnectionHeadResponse::Busy, None)
				}
			}
		};

		let head_response = HeadResponse::JobConnection(response);
		log::debug!("{:?}", head_response);

		match write_cbor(&mut stream, &head_response).await.unwrap() {
			Ok(_) => {}
			Err(io_error) => {
				log::warn!("While writing head response: {io_error:?}");
				return;
			}
		}

		if let Some((module, nonce)) = payload {
			// Now, we'll send an actual connection request to the module.
			//

			let connection_id = {
				let mut connection_id_lock =
					module.job_connections_counter.lock().await;

				*connection_id_lock += 1;
				*connection_id_lock
			};

			let (envelope, _) = rpc::OutboundRequestEnvelope::new(
				rpc::OutboundRequestFrameData::ProviderOpenJobConnection {
					connection_id,
					nonce,
				},
			);

			module
				.future_job_connections
				.lock()
				.await
				.insert(connection_id, stream.into_inner());

			module
				.outbound_request_tx
				.try_send(envelope)
				.expect("module's outbound_request_tx should be healthy");
		}
	}
}
