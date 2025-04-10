use serde::{Deserialize, Serialize};

use crate::rpc::{
	connection::Connection,
	procedure::{
		OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
	},
};

/// Query system state.
#[derive(Deserialize, Debug)]
pub struct SystemQueryRequest {}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum SystemQueryResponse {
	Ok { peer_id: libp2p::PeerId },
}

impl Connection {
	pub async fn handle_system_query_request(
		&self,
		request_id: u32,
		_request_data: &SystemQueryRequest,
	) {
		let response = SystemQueryResponse::Ok {
			peer_id: self.state.p2p.lock().await.keypair.public().to_peer_id(),
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::QuerySystem(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
