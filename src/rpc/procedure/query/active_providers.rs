use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{
	database::providers::query_active_providers,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
};

/// Query peer IDs of providers with recent heartbeats.
#[derive(Deserialize, Debug)]
pub struct ActiveProvidersQueryRequest {}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ActiveProvidersQueryResponse {
	Ok(Vec<String>),
}

impl Connection {
	pub async fn handle_active_providers_query_request(
		&self,
		request_id: u32,
		_request_data: &ActiveProvidersQueryRequest,
	) {
		let response = ActiveProvidersQueryResponse::Ok(query_active_providers(
			&*self.state.database.lock().await,
			Duration::from_secs(60),
		));

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::QueryActiveProviders(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
