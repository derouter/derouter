use libp2p::gossipsub::TopicHash;

use super::{Node, proto};

pub mod heartbeat;

#[derive(Clone)]
pub enum Topic {
	Ident(libp2p::gossipsub::IdentTopic),

	#[allow(dead_code)]
	Sha256(libp2p::gossipsub::Sha256Topic),
}

impl Topic {
	pub fn hash(&self) -> TopicHash {
		match self {
			Self::Ident(t) => t.hash(),
			Self::Sha256(t) => t.hash(),
		}
	}
}

impl From<Topic> for TopicHash {
	fn from(val: Topic) -> Self {
		val.hash()
	}
}

impl Node {
	pub async fn handle_gossipsub_event(
		&mut self,
		event: libp2p::gossipsub::Event,
	) {
		match event {
			libp2p::gossipsub::Event::Message { message, .. } => {
				if message.topic == self.heartbeat_topic.hash() {
					if let Some(source) = message.source {
						match serde_cbor::from_slice::<proto::gossipsub::Heartbeat>(
							&message.data,
						) {
							Ok(heartbeat) => {
								log::trace!("🫀 Got {:?}", heartbeat);

								if let Err(e) = self.handle_heartbeat(source, heartbeat).await {
									log::warn!("{:?}", e);
								}
							}

							Err(e) => {
								log::warn!("{:?}", e)
							}
						}
					}
				} else {
					log::warn!("Unknown topic {}", message.topic)
				}
			}

			libp2p::gossipsub::Event::Subscribed { .. } => {
				log::debug!("{:?}", event)
			}

			libp2p::gossipsub::Event::Unsubscribed { .. } => {
				log::debug!("{:?}", event)
			}

			libp2p::gossipsub::Event::GossipsubNotSupported { .. } => {
				log::debug!("{:?}", event)
			}

			libp2p::gossipsub::Event::SlowPeer { .. } => {
				log::debug!("{:?}", event)
			}
		};
	}
}
