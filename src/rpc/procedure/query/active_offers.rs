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
	provider_peer_ids: Option<Vec<libp2p::PeerId>>,
}

#[derive(Serialize, derive_more::Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ActiveOffersQueryResponse {
	Ok(#[debug(skip)] Vec<OfferSnapshot>),
}

impl Connection {
	pub async fn handle_active_offers_query_request(
		&self,
		request_id: u32,
		request_data: &ActiveOffersQueryRequest,
	) {
		let response = ActiveOffersQueryResponse::Ok(query_active_offers(
			&*self.state.db.lock().await,
			request_data.protocol_ids.as_deref(),
			request_data.provider_peer_ids.as_deref(),
		));

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::QueryActiveOffers(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
