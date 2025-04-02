use serde::{Deserialize, Serialize};

use crate::rpc::{
	connection::Connection,
	procedure::{
		OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
	},
};

/// Subscribe to active offer updates.
#[derive(Deserialize, Debug)]
pub struct CancelSubscriptionRequest {
	subscription_id: u32,
}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum CancelSubscriptionResponse {
	Ok,
	InvalidSubscriptionId,
}

impl Connection {
	pub async fn handle_cancel_subscription_request(
		&mut self,
		request_id: u32,
		request_data: &CancelSubscriptionRequest,
	) {
		let response =
			match self.subscriptions.remove(&request_data.subscription_id) {
				Some(_) => CancelSubscriptionResponse::Ok,
				None => CancelSubscriptionResponse::InvalidSubscriptionId,
			};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::CancelSubscription(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
