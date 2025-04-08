use std::str::FromStr;

use either::Either::Left;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use tokio_util::compat::FuturesAsyncReadCompatExt;

use crate::{
	db::{create_service_connection, service_connections::Currency},
	p2p,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
	util::cbor::read_cbor,
};

/// Open a new connection for given offer.
#[derive(Deserialize, Debug)]
pub struct ConsumerOpenConnectionRequest {
	/// Local offer snapshot ID.
	offer_snapshot_id: i64,

	/// Currency enum.
	currency: Currency,
}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ConsumerOpenConnectionResponse {
	Ok {
		/// Local service connection ID.
		connection_id: i64,
	},

	ProviderUnreacheableError,
	ProviderOfferNotFoundError,

	OtherRemoteError(String),
	OtherLocalError(String),
}

impl Connection {
	pub async fn handle_consumer_open_connection_request(
		&self,
		request_id: u32,
		request_data: ConsumerOpenConnectionRequest,
	) {
		let conn = self.state.db.lock().await;

		#[derive(Clone)]
		struct OfferSnapshot {
			provider_peer_id: libp2p::PeerId,
			protocol_id: String,
			offer_id: String,
			protocol_payload: String,
		}

		// REFACTOR: Move to the database mod.
		let offer_snapshot = conn
			.query_row(
				r#"
				SELECT
					provider_peer_id, -- #0
					protocol_id,      -- #1
					offer_id,         -- #2
					protocol_payload  -- #3
				FROM
					offer_snapshots
				WHERE
					ROWID = ?1 AND
					active = 1
			"#,
				[request_data.offer_snapshot_id],
				|row| {
					Ok(OfferSnapshot {
						provider_peer_id: libp2p::PeerId::from_str(
							row.get_ref(0).unwrap().as_str().unwrap(),
						)
						.unwrap(),
						protocol_id: row.get(1)?,
						offer_id: row.get(2)?,
						protocol_payload: row.get(3)?,
					})
				},
			)
			.optional()
			.unwrap();

		drop(conn);

		if let Some(offer_snapshot) = offer_snapshot {
			let provider_peer_id = offer_snapshot.provider_peer_id;

			let outbound_tx = self.outbound_tx.clone();
			let state = self.state.clone();
			let future_connections = self.future_connections.clone();

			tokio::spawn(async move {
				let (p2p_stream_tx, p2p_stream_rx) = tokio::sync::oneshot::channel::<
					p2p::stream::OutboundStreamRequestResult,
				>();

				let stream_request_tx =
					state.p2p.lock().await.stream_request_tx.clone();

				stream_request_tx
					.try_send(p2p::stream::OutboundStreamRequest {
						target_peer_id: provider_peer_id,
						head_request: p2p::proto::stream::HeadRequest::ServiceConnection {
							protocol_id: offer_snapshot.protocol_id,
							offer_id: offer_snapshot.offer_id,
							protocol_payload: offer_snapshot.protocol_payload,
							currency: request_data.currency,
						},
						result_tx: p2p_stream_tx,
					})
					.unwrap(); // Otherwise the program is not healthy.

				let open_connection_response = match p2p_stream_rx.await.unwrap() {
					Ok((consumer_peer_id, stream)) => {
						// Here, we've opened & written header to the P2P stream.
						// Shall check for the header response.
						//

						let mut stream = stream.compat();

						match read_cbor::<_, p2p::proto::stream::HeadResponse>(&mut stream)
							.await
						{
							Ok(Some(head_response)) => {
								match head_response {
									p2p::proto::stream::HeadResponse::ServiceConnection(
										service_connection_head_response,
									) => {
										type ServiceConnectionHeadResponse =
											p2p::proto::stream::ServiceConnectionHeadResponse;

										match service_connection_head_response {
											ServiceConnectionHeadResponse::Ok => {
												// We'll now create a local service connection.
												//

												let (_, connection_rowid) = create_service_connection(
													&mut *state.db.lock().await,
													Left(request_data.offer_snapshot_id),
													consumer_peer_id,
													request_data.currency,
												);

												future_connections
													.lock()
													.await
													.insert(connection_rowid, stream.into_inner());

												// Finally!
												ConsumerOpenConnectionResponse::Ok {
													connection_id: connection_rowid,
												}
											}
											ServiceConnectionHeadResponse::OfferNotFoundError => {
												ConsumerOpenConnectionResponse::ProviderOfferNotFoundError
											}
											ServiceConnectionHeadResponse::AnotherError(e) => {
												ConsumerOpenConnectionResponse::OtherRemoteError(e)
											}
										}
									}
								}
							}

							Ok(None) => {
								log::debug!(
									"P2P stream EOF'ed before we could read head response"
								);

								ConsumerOpenConnectionResponse::ProviderUnreacheableError
							}

							Err(e) => {
								log::warn!(
									"While reading head response from P2P stream: {:?}",
									e
								);

								ConsumerOpenConnectionResponse::OtherLocalError(format!(
									"{}",
									e
								))
							}
						}
					}

					Err(e) => {
						log::warn!("{:?}", e);
						ConsumerOpenConnectionResponse::ProviderUnreacheableError
					}
				};

				let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
					id: request_id,
					data: OutboundResponseFrameData::ConsumerOpenConnection(
						open_connection_response,
					),
				});

				outbound_tx.send(outbound_frame).await.unwrap();
			});
		} else {
			log::warn!(
				"Could not find offer snapshot locally by ID {}",
				request_data.offer_snapshot_id
			);

			let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
				id: request_id,
				data: OutboundResponseFrameData::ConsumerOpenConnection(
					ConsumerOpenConnectionResponse::OtherLocalError(
						"Could not find offer snapshot locally".to_owned(),
					),
				),
			});

			self.outbound_tx.send(outbound_frame).await.unwrap();
		}
	}
}
