use std::{
	fs::create_dir_all,
	hash::{DefaultHasher, Hash as _, Hasher as _},
	path::Path,
	sync::Arc,
	time::Duration,
};

use futures::StreamExt as _;
use handle_heartbeat::handle_heartbeat;
use libp2p::{
	StreamProtocol, Swarm, SwarmBuilder, gossipsub,
	identity::Keypair,
	mdns, noise, ping, request_response,
	swarm::{self, SwarmEvent},
	tcp, yamux,
};

use crate::state::SharedState;

mod handle_heartbeat;
pub mod proto;

// const STREAM_PROTOCOL: &str = "/derouter/stream/0.1.0";
const REQUEST_RESPONSE_PROTOCOL: &str = "/derouter/reqres/0.1.0";

pub fn read_or_create_keypair(keypair_path: &Path) -> eyre::Result<Keypair> {
	log::debug!("🔑 Reading keypair from {}", keypair_path.display());

	if let Ok(read) = std::fs::read(keypair_path) {
		Ok(libp2p::identity::Keypair::from_protobuf_encoding(&read)?)
	} else {
		log::warn!(
			"Failed to read Keypair from {}, generating new one..",
			keypair_path.display()
		);

		let keypair = libp2p::identity::Keypair::generate_ed25519();
		let encoded = keypair.to_protobuf_encoding()?;

		create_dir_all(keypair_path.parent().unwrap())?;
		std::fs::write(keypair_path, &encoded)?;

		Ok(keypair)
	}
}

#[derive(swarm::NetworkBehaviour)]
pub struct NodeBehaviour {
	mdns: mdns::tokio::Behaviour,
	ping: ping::Behaviour,
	request_response: request_response::cbor::Behaviour<
		proto::request_response::Request,
		proto::request_response::Response,
	>,
	stream: libp2p_stream::Behaviour,
	gossipsub: gossipsub::Behaviour,
}

pub async fn run_p2p(state: Arc<SharedState>) -> eyre::Result<()> {
	let keypair = read_or_create_keypair(&state.config.keypair_path)?;

	let mdns = mdns::tokio::Behaviour::new(
		mdns::Config::default(),
		keypair.public().to_peer_id(),
	)?;

	let mut swarm = SwarmBuilder::with_existing_identity(keypair.clone())
		.with_tokio()
		.with_tcp(
			tcp::Config::default().nodelay(true),
			noise::Config::new,
			yamux::Config::default,
		)?
		.with_quic()
		.with_dns()?
		.with_behaviour(|keypair| {
			// To content-address message, we can take
			// the hash of message and use it as an ID.
			let message_id_fn = |message: &gossipsub::Message| {
				let mut s = DefaultHasher::new();
				message.data.hash(&mut s);
				gossipsub::MessageId::from(s.finish().to_string())
			};

			let gossipsub_config = gossipsub::ConfigBuilder::default()
				// This is set to aid debugging by not cluttering the log space.
				.heartbeat_interval(Duration::from_secs(10))
				// This sets the kind of message validation.
				// The default is Strict (enforce message signing).
				.validation_mode(gossipsub::ValidationMode::Strict)
				.message_id_fn(message_id_fn)
				.build()?;

			let gossipsub = gossipsub::Behaviour::new(
				gossipsub::MessageAuthenticity::Signed(keypair.clone()),
				gossipsub_config,
			)?;

			Ok(NodeBehaviour {
				mdns,
				ping: ping::Behaviour::new(ping::Config::new()),
				request_response: request_response::cbor::Behaviour::<
					proto::request_response::Request,
					proto::request_response::Response,
				>::new(
					[(
						StreamProtocol::new(REQUEST_RESPONSE_PROTOCOL),
						request_response::ProtocolSupport::Full,
					)],
					request_response::Config::default(),
				),
				stream: libp2p_stream::Behaviour::new(),
				gossipsub,
			})
		})?
		.build();

	swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;
	swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

	let mut provider_heartbeat_interval =
		tokio::time::interval(std::time::Duration::from_secs(30));

	provider_heartbeat_interval
		.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

	let heartbeat_topic = gossipsub::IdentTopic::new("heartbeat");

	swarm
		.behaviour_mut()
		.gossipsub
		.subscribe(&heartbeat_topic)?;

	log::info!("📡 Running w/ PeerID {}", swarm.local_peer_id());

	loop {
		#[rustfmt::skip]
		tokio::select! {
		  event = swarm.next() => {
        if let Some(event) = event {
          handle_event(&state, &mut swarm, event, heartbeat_topic.clone()).await;
        } else {
          log::debug!("Empty swarm event, breaking loop");
          break;
        }
		  }

      _ = provider_heartbeat_interval.tick() => {
        try_send_heartbeat(&state, &mut swarm, heartbeat_topic.clone()).await?;
      }

      _ = state.shutdown_token.cancelled() => {
        log::info!("🛑 Shutting down...");
				break;
			}
		}
	}

	log::debug!("✅ Exited event loop");

	Ok(())
}

async fn try_send_heartbeat<T: Into<gossipsub::TopicHash>>(
	state: &SharedState,
	swarm: &mut Swarm<NodeBehaviour>,
	topic: T,
) -> eyre::Result<()> {
	let lock = state.provider.lock().await;

	if lock.actual_offers.is_empty() {
		log::debug!("No provided offers, skip heartbeat");
		return Ok(());
	}

	let message = proto::gossipsub::Heartbeat {
		provider: Some(proto::gossipsub::ProviderDetails {
			name: state.config.provider.name.clone(),
			teaser: state.config.provider.teaser.clone(),
			description: state.config.provider.description.clone(),
			offers: lock.actual_offers.clone(),
			updated_at: lock.last_updated_at,
		}),
		timestamp: chrono::Utc::now(),
	};

	log::debug!("Sending {:?}", message);
	let buffer = serde_cbor::to_vec(&message).unwrap();

	match swarm.behaviour_mut().gossipsub.publish(topic, buffer) {
		Ok(_) => {
			log::debug!("🫀 Sent heartbeat");
			Ok(())
		}

		Err(error) => match error {
			gossipsub::PublishError::Duplicate => {
				log::warn!("try_send_heartbeat: {}", error);
				Ok(())
			}

			gossipsub::PublishError::SigningError(error) => Err(error.into()),

			gossipsub::PublishError::InsufficientPeers => {
				log::warn!("try_send_heartbeat: {}", error);
				Ok(())
			}

			gossipsub::PublishError::MessageTooLarge => Err(error.into()),
			gossipsub::PublishError::TransformFailed(error) => Err(error.into()),

			gossipsub::PublishError::AllQueuesFull(_) => {
				log::warn!("try_send_heartbeat: {}", error);
				Ok(())
			}
		},
	}
}

async fn handle_event<T: Into<gossipsub::TopicHash>>(
	state: &SharedState,
	swarm: &mut Swarm<NodeBehaviour>,
	event: SwarmEvent<NodeBehaviourEvent>,
	heartbeat_topic: T,
) {
	match &event {
		SwarmEvent::Behaviour(event) => match event {
			NodeBehaviourEvent::Mdns(event) => match event {
				mdns::Event::Discovered(items) => {
					for (peer_id, address) in items {
						log::debug!(
							"👀 New mDNS address discovered: {} {}",
							peer_id,
							address
						);
					}
				}

				mdns::Event::Expired(items) => {
					for (peer_id, address) in items {
						log::debug!("💩 mDNS address expired: {} {}", peer_id, address);
					}
				}
			},

			NodeBehaviourEvent::Ping(event) => {
				log::trace!("{:?}", event)
			}

			NodeBehaviourEvent::RequestResponse(event) => {
				match event {
					request_response::Event::Message { .. } => todo!(),
					request_response::Event::OutboundFailure { .. } => todo!(),
					request_response::Event::InboundFailure { .. } => todo!(),
					request_response::Event::ResponseSent { .. } => todo!(),
				};
			}

			NodeBehaviourEvent::Stream(_) => todo!(),

			NodeBehaviourEvent::Gossipsub(event) => {
				match event {
					gossipsub::Event::Message { message, .. } => {
						if message.topic == heartbeat_topic.into() {
							if let Some(source) = message.source {
								match serde_cbor::from_slice::<proto::gossipsub::Heartbeat>(
									&message.data,
								) {
									Ok(heartbeat) => {
										log::debug!("🫀 Got {:?}", heartbeat);

										if let Err(e) =
											handle_heartbeat(state, source, heartbeat).await
										{
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

					gossipsub::Event::Subscribed { .. } => {
						log::debug!("{:?}", event)
					}

					gossipsub::Event::Unsubscribed { .. } => {
						log::debug!("{:?}", event)
					}

					gossipsub::Event::GossipsubNotSupported { .. } => {
						log::debug!("{:?}", event)
					}

					gossipsub::Event::SlowPeer { .. } => {
						log::debug!("{:?}", event)
					}
				};
			}
		},

		SwarmEvent::ConnectionEstablished { .. } => {
			log::debug!("✅ {:?}", event)
		}

		SwarmEvent::ConnectionClosed { .. } => {
			log::debug!("👋 {:?}", event)
		}

		SwarmEvent::IncomingConnection { .. } => {
			log::trace!("{:?}", event)
		}

		SwarmEvent::IncomingConnectionError { .. } => {
			log::warn!("{:?}", event)
		}

		SwarmEvent::OutgoingConnectionError { .. } => {
			log::warn!("{:?}", event)
			// swarm.behaviour_mut().gossipsub.remove_explicit_peer(peer_id);
		}

		SwarmEvent::NewListenAddr { .. } => {
			log::debug!("{:?}", event);
		}

		SwarmEvent::ExpiredListenAddr { .. } => {
			log::debug!("{:?}", event);
		}

		SwarmEvent::ListenerClosed { .. } => todo!(),
		SwarmEvent::ListenerError { .. } => todo!(),

		SwarmEvent::Dialing { .. } => {
			log::trace!("{:?}", event)
		}

		SwarmEvent::NewExternalAddrCandidate { .. } => todo!(),
		SwarmEvent::ExternalAddrConfirmed { .. } => todo!(),
		SwarmEvent::ExternalAddrExpired { .. } => todo!(),

		SwarmEvent::NewExternalAddrOfPeer { peer_id, .. } => {
			log::trace!("{:?}", event);
			swarm.behaviour_mut().gossipsub.add_explicit_peer(peer_id);
		}

		_ => {
			log::error!("Unhandled {:?}", event)
		}
	}
}
