use serde::{Deserialize, Serialize};
use tokio_util::compat::FuturesAsyncReadCompatExt;

use crate::{
	db::{self, service_jobs::get::find_by_job_id},
	p2p,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
	util::cbor::read_cbor,
};

/// Open a new connection for given job.
#[derive(Deserialize, Debug)]
pub struct ConsumerOpenJobConnectionRequest {
	provider_peer_id: libp2p::PeerId,
	provider_job_id: String,
}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ConsumerOpenJobConnectionResponse {
	Ok {
		/// Local job connection ID.
		connection_id: u64,
	},

	/// The job was not found locally.
	LocalJobNotFound,

	/// Provider is unreacheable via P2P network.
	ProviderUnreacheable,

	/// Provider claims they don't have this job.
	ProviderJobNotFound,

	/// Provider found the job, but refused to open
	/// a connection to it due to "expiration".
	ProviderJobExpired,

	/// Provider tells us that they're temporarily busy.
	ProviderBusy,

	OtherRemoteError(String),
	OtherLocalError(String),
}

impl Connection {
	pub async fn handle_consumer_open_job_connection_request(
		&self,
		request_id: u32,
		request_data: ConsumerOpenJobConnectionRequest,
	) {
		let job = find_by_job_id(
			db::ConnectionLike::Connection(&*self.state.db.lock().await),
			request_data.provider_peer_id,
			&request_data.provider_job_id,
		);

		if let Some(job) = job {
			let outbound_tx = self.outbound_tx.clone();
			let state = self.state.clone();
			let future_connection_counter = self.future_connection_counter.clone();
			let future_connections = self.future_connections.clone();

			tokio::spawn(async move {
				let (p2p_stream_tx, p2p_stream_rx) = tokio::sync::oneshot::channel::<
					p2p::stream::OutboundStreamRequestResult,
				>();

				let stream_request_tx =
					state.p2p.lock().await.stream_request_tx.clone();

				stream_request_tx
					.try_send(p2p::stream::OutboundStreamRequest {
						target_peer_id: job.provider_peer_id,
						head_request: p2p::proto::stream::HeadRequest::JobConnection(
							p2p::proto::stream::JobConnectionHeadRequest {
								provider_job_id: job.provider_job_id,
							},
						),
						result_tx: p2p_stream_tx,
					})
					.unwrap(); // Otherwise the program is not healthy.

				let open_connection_response = match p2p_stream_rx.await.unwrap() {
					Ok((_, stream)) => {
						// Here, we've opened & written header to the P2P stream.
						// Shall check for the header response.
						//

						let mut stream = stream.compat();

						match read_cbor::<_, p2p::proto::stream::HeadResponse>(&mut stream)
							.await
						{
							Ok(Some(head_response)) => {
								match head_response {
									p2p::proto::stream::HeadResponse::JobConnection(
										service_connection_head_response,
									) => {
										type ServiceConnectionHeadResponse =
											p2p::proto::stream::JobConnectionHeadResponse;

										match service_connection_head_response {
											ServiceConnectionHeadResponse::Ok => {
												let mut connection_id =
													future_connection_counter.lock().await;

												*connection_id += 1;

												future_connections
													.lock()
													.await
													.insert(*connection_id, stream.into_inner());

												// Finally!
												ConsumerOpenJobConnectionResponse::Ok {
													connection_id: *connection_id,
												}
											}

											ServiceConnectionHeadResponse::JobNotFound => {
												ConsumerOpenJobConnectionResponse::ProviderJobNotFound
											}

											ServiceConnectionHeadResponse::JobExpired => {
												ConsumerOpenJobConnectionResponse::ProviderJobExpired
											}

											ServiceConnectionHeadResponse::Busy => {
												ConsumerOpenJobConnectionResponse::ProviderBusy
											}

											ServiceConnectionHeadResponse::AnotherError(e) => {
												ConsumerOpenJobConnectionResponse::OtherRemoteError(e)
											}
										}
									}
								}
							}

							Ok(None) => {
								log::debug!(
									"P2P stream EOF'ed before we could read head response"
								);

								ConsumerOpenJobConnectionResponse::ProviderUnreacheable
							}

							Err(e) => {
								log::warn!(
									"While reading head response from P2P stream: {:?}",
									e
								);

								ConsumerOpenJobConnectionResponse::OtherLocalError(format!(
									"{}",
									e
								))
							}
						}
					}

					Err(e) => {
						log::warn!("{:?}", e);
						ConsumerOpenJobConnectionResponse::ProviderUnreacheable
					}
				};

				let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
					id: request_id,
					data: OutboundResponseFrameData::ConsumerOpenJobConnection(
						open_connection_response,
					),
				});

				outbound_tx.send(outbound_frame).await.unwrap();
			});
		} else {
			let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
				id: request_id,
				data: OutboundResponseFrameData::ConsumerOpenJobConnection(
					ConsumerOpenJobConnectionResponse::LocalJobNotFound,
				),
			});

			self.outbound_tx.send(outbound_frame).await.unwrap();
		}
	}
}
