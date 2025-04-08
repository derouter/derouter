use std::collections::HashMap;

use unwrap_none::UnwrapNone;

use crate::p2p::{Node, proto};

impl Node {
	pub async fn maybe_send_heartbeat(
		&mut self,
	) -> eyre::Result<Option<()>, libp2p::gossipsub::PublishError> {
		let provider_lock = self.state.provider.lock().await;

		let mut any_offers = false;

		for module in provider_lock.modules.values() {
			if !module.offers.is_empty() {
				any_offers = true;
				break;
			}
		}

		if !any_offers {
			log::debug!("No provided offers, skip heartbeat");
			return Ok(None);
		}

		let mut heartbeat_offers =
			HashMap::<String, HashMap<String, proto::gossipsub::ProviderOffer>>::new(
			);

		for module in provider_lock.modules.values() {
			for (protocol_id, provided_offers_by_protocol) in &module.offers {
				let heartbeat_offers_by_protocol =
					match heartbeat_offers.get_mut(protocol_id) {
						Some(map) => map,
						None => {
							heartbeat_offers.insert(protocol_id.clone(), HashMap::new());
							heartbeat_offers.get_mut(protocol_id).unwrap()
						}
					};

				for (offer_id, provided_offer) in provided_offers_by_protocol {
					heartbeat_offers_by_protocol
						.insert(
							offer_id.clone(),
							proto::gossipsub::ProviderOffer {
								protocol_payload: provided_offer.protocol_payload.clone(),
							},
						)
						.unwrap_none();
				}
			}
		}

		let message = proto::gossipsub::Heartbeat {
			provider: Some(proto::gossipsub::ProviderDetails {
				name: self.state.config.provider.name.clone(),
				teaser: self.state.config.provider.teaser.clone(),
				description: self.state.config.provider.description.clone(),
				offers: heartbeat_offers,
				updated_at: provider_lock.last_updated_at,
			}),
			timestamp: chrono::Utc::now(),
		};

		log::trace!("Sending {:?}", message);
		let buffer = serde_cbor::to_vec(&message).unwrap();

		match self
			.swarm
			.behaviour_mut()
			.gossipsub
			.publish(self.heartbeat_topic.clone(), buffer)
		{
			Ok(_) => {
				log::debug!("🫀 Sent heartbeat at {}", message.timestamp);
				Ok(Some(()))
			}

			Err(error) => match error {
				libp2p::gossipsub::PublishError::SigningError(_) => Err(error),
				libp2p::gossipsub::PublishError::MessageTooLarge => Err(error),
				libp2p::gossipsub::PublishError::TransformFailed(_) => Err(error),
				libp2p::gossipsub::PublishError::Duplicate => Err(error),

				libp2p::gossipsub::PublishError::InsufficientPeers => {
					log::warn!("{error}");
					Ok(None)
				}

				libp2p::gossipsub::PublishError::AllQueuesFull(_) => {
					log::warn!("{error}");
					Ok(None)
				}
			},
		}
	}
}
