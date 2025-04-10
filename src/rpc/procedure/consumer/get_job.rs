use serde::{Deserialize, Serialize};

use crate::{
	p2p::{self, reqres::OutboundRequestEnvelope},
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
	util::format_secret_option,
};

/// Get job information from the according provider.
#[derive(Deserialize, derive_more::Debug)]
pub struct ConsumerGetJobRequest {
	provider_peer_id: libp2p::PeerId,
	provider_job_id: String,
}

#[derive(Serialize, derive_more::Debug)]
pub enum ConsumerGetJobResponse {
	Ok {
		/// Publicly-available job payload, if completed.
		#[debug("{}", format_secret_option(public_payload))]
		public_payload: Option<String>,

		/// Publicly-available balance delta in [Currency]-specific encoding.
		balance_delta: Option<String>,

		/// Publicly-available job completion timestamp, as told by the Provider.
		created_at_sync: i64,

		/// Publicly-available job completion timestamp, as told by the Provider.
		completed_at_sync: Option<i64>,
	},

	ProviderUnreacheable,

	/// May also indicate authorization error.
	JobNotFound,
}

impl Connection {
	pub async fn handle_consumer_get_job_request(
		&self,
		request_id: u32,
		request_data: ConsumerGetJobRequest,
	) {
		let (envelope, response_rx) = OutboundRequestEnvelope::new(
			p2p::proto::request_response::Request::GetJob(
				p2p::proto::request_response::GetJobRequest {
					provider_job_id: request_data.provider_job_id,
				},
			),
			request_data.provider_peer_id,
		);

		self
			.state
			.p2p
			.lock()
			.await
			.reqres_request_tx
			.send(envelope)
			.await
			.unwrap();

		let response = {
			match response_rx.await.unwrap() {
				Ok(response) => {
					type R = p2p::proto::request_response::Response;

					match response {
						R::GetJob(get_job_response) => {
							type R = p2p::proto::request_response::GetJobResponse;

							match get_job_response {
								R::Ok {
									public_payload,
									balance_delta,
									created_at_sync,
									completed_at_sync,
								} => ConsumerGetJobResponse::Ok {
									public_payload,
									balance_delta,
									created_at_sync,
									completed_at_sync,
								},
								R::JobNotFound => ConsumerGetJobResponse::JobNotFound,
							}
						}

						unexpected => {
							log::warn!(
								"Unexpected P2P response \
                  from Provider: {unexpected:?}"
							);

							ConsumerGetJobResponse::ProviderUnreacheable
						}
					}
				}

				Err(outbound_failure) => {
					log::warn!("{outbound_failure:?}");
					ConsumerGetJobResponse::ProviderUnreacheable
				}
			}
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::ConsumerGetJob(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
