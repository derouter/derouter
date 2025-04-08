use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{
	database::providers::query_providers_by_peer_id,
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
	provider_peer_ids: Vec<String>,
}

#[derive(Serialize, derive_more::Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ProvidersQueryResponse {
	Ok(#[debug(skip)] Vec<ProviderRecord>),
	InvalidPeerId(String),
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

			let provider_peer_ids = {
				let mut parsed_peer_ids = vec![];

				for raw_peer_id in &request_data.provider_peer_ids {
					parsed_peer_ids.push(match libp2p::PeerId::from_str(raw_peer_id) {
						Ok(x) => x,
						Err(_) => {
							break 'block ProvidersQueryResponse::InvalidPeerId(
								raw_peer_id.to_string(),
							);
						}
					});
				}

				parsed_peer_ids
			};

			ProvidersQueryResponse::Ok(query_providers_by_peer_id(
				&*self.state.database.lock().await,
				provider_peer_ids.as_ref(),
			))
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::QueryProviders(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
