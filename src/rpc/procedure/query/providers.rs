use serde::{Deserialize, Serialize};

use crate::{
	db::providers::query_providers_by_peer_id,
	dto::ProviderRecord,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
};

/// Query providers by their peer IDs.
#[derive(Deserialize, Debug)]
pub struct ProvidersQueryRequest {
	provider_peer_ids: Vec<libp2p::PeerId>,
}

#[derive(Serialize, derive_more::Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ProvidersQueryResponse {
	Ok(#[debug(skip)] Vec<ProviderRecord>),
	Length,
}

impl Connection {
	pub async fn handle_providers_query_request(
		&self,
		request_id: u32,
		request_data: &ProvidersQueryRequest,
	) {
		let response = 'block: {
			if request_data.provider_peer_ids.len() > 100 {
				break 'block ProvidersQueryResponse::Length;
			}

			ProvidersQueryResponse::Ok(query_providers_by_peer_id(
				&*self.state.db.lock().await,
				request_data.provider_peer_ids.as_ref(),
			))
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::QueryProviders(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
