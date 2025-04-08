use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{
	db::offers::query_active_offers,
	dto::OfferSnapshot,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
};

/// Get all active offers.
#[derive(Deserialize, Debug)]
pub struct ActiveOffersQueryRequest {
	/// Optionally filter by given protocol IDs.
	protocol_ids: Option<Vec<String>>,

	/// Optionally filter by given provider peer IDs.
	provider_peer_ids: Option<Vec<String>>,
}

#[derive(Serialize, derive_more::Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ActiveOffersQueryResponse {
	Ok(#[debug(skip)] Vec<OfferSnapshot>),
	InvalidPeerId(String),
}

impl Connection {
	pub async fn handle_active_offers_query_request(
		&self,
		request_id: u32,
		request_data: &ActiveOffersQueryRequest,
	) {
		let response = 'block: {
			let provider_peer_ids =
				if let Some(raw_peer_ids) = &request_data.provider_peer_ids {
					let mut parsed_peer_ids = vec![];

					for raw_peer_id in raw_peer_ids {
						parsed_peer_ids.push(match libp2p::PeerId::from_str(raw_peer_id) {
							Ok(x) => x,
							Err(_) => {
								break 'block ActiveOffersQueryResponse::InvalidPeerId(
									raw_peer_id.to_string(),
								);
							}
						});
					}

					Some(parsed_peer_ids)
				} else {
					None
				};

			ActiveOffersQueryResponse::Ok(query_active_offers(
				&*self.state.db.lock().await,
				request_data.protocol_ids.as_deref(),
				provider_peer_ids.as_deref(),
			))
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::QueryActiveOffers(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
