use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::rpc::{
	connection::{Connection, Subscription},
	procedure::{
		OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
	},
};

/// Subscribe to active offer updates.
#[derive(Deserialize, Debug)]
pub struct ActiveOffersSubscriptionRequest {
	/// Optionally filter by given protocol IDs.
	protocol_ids: Option<Vec<String>>,

	/// Optionally filter by given provider peer IDs.
	provider_peer_ids: Option<Vec<String>>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ActiveOffersSubscriptionResponse {
	Ok(u32),
	InvalidPeerId(String),
}

impl Connection {
	pub async fn handle_active_offers_subscription_request(
		&mut self,
		request_id: u32,
		request_data: &ActiveOffersSubscriptionRequest,
	) {
		let response = 'block: {
			let provider_peer_ids =
				if let Some(raw_peer_ids) = &request_data.provider_peer_ids {
					let mut parsed_peer_ids = vec![];

					for raw_peer_id in raw_peer_ids {
						parsed_peer_ids.push(match libp2p::PeerId::from_str(raw_peer_id) {
							Ok(x) => x,
							Err(_) => {
								break 'block ActiveOffersSubscriptionResponse::InvalidPeerId(
									raw_peer_id.to_string(),
								);
							}
						});
					}

					Some(parsed_peer_ids)
				} else {
					None
				};

			let subscription_id = self.subscriptions_counter;
			self.subscriptions_counter += 1;

			self.subscriptions.insert(
				subscription_id,
				Subscription::ActiveOffers {
					protocol_ids: request_data.protocol_ids.clone(),
					provider_peer_ids,
				},
			);

			ActiveOffersSubscriptionResponse::Ok(subscription_id)
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::SubscribeToActiveOffers(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
