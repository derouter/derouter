use serde::{Deserialize, Serialize};

use crate::rpc::{
	connection::{Connection, Subscription},
	procedure::{
		OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
	},
};

/// Subscribe to active offer updates.
#[derive(Deserialize, Debug)]
pub struct ActiveProvidersSubscriptionRequest {}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ActiveProvidersSubscriptionResponse {
	Ok(u32),
}

impl Connection {
	pub async fn handle_active_providers_subscription_request(
		&mut self,
		request_id: u32,
		_request_data: &ActiveProvidersSubscriptionRequest,
	) {
		let response = {
			let subscription_id = self.subscriptions_counter;
			self.subscriptions_counter += 1;

			self
				.subscriptions
				.insert(subscription_id, Subscription::ActiveProviders);

			ActiveProvidersSubscriptionResponse::Ok(subscription_id)
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::SubscribeToActiveProviders(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
