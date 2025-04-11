use std::{
	collections::{HashMap, HashSet},
	fs::create_dir_all,
	hash::{DefaultHasher, Hash as _, Hasher as _},
	path::Path,
	sync::Arc,
	time::Duration,
};

use futures::StreamExt as _;
use job_confirmation_scheduler::job_confirmation_loop;
use libp2p::{
	StreamProtocol, Swarm, SwarmBuilder, dcutr, identify,
	identity::Keypair,
	mdns, noise, ping, relay,
	swarm::{NetworkBehaviour, SwarmEvent},
	tcp, yamux,
};

use crate::state::SharedState;

pub mod gossipsub;
mod job_confirmation_scheduler;
pub mod proto;
pub mod reqres;
pub mod stream;

const IDENTIFY_PROTOCOL: &str = "/derouter/identify/0.1.0";
const STREAM_PROTOCOL: &str = "/derouter/stream/0.1.0";
const REQUEST_RESPONSE_PROTOCOL: &str = "/derouter/reqres/0.1.0";

pub fn read_or_create_keypair(keypair_path: &Path) -> eyre::Result<Keypair> {
	log::debug!("🔑 Reading keypair from {}", keypair_path.display());

	if let Ok(read) = std::fs::read(keypair_path) {
		Ok(Keypair::from_protobuf_encoding(&read)?)
	} else {
		log::warn!(
			"Failed to read Keypair from {}, generating new one..",
			keypair_path.display()
		);

		let keypair = Keypair::generate_ed25519();
		let encoded = keypair.to_protobuf_encoding()?;

		create_dir_all(keypair_path.parent().unwrap())?;
		std::fs::write(keypair_path, &encoded)?;

		Ok(keypair)
	}
}

#[derive(NetworkBehaviour)]
pub struct NodeBehaviour {
	mdns: mdns::tokio::Behaviour,
	ping: ping::Behaviour,
	identify: identify::Behaviour,
	relay_client: relay::client::Behaviour,
	dcutr: dcutr::Behaviour,
	request_response: libp2p::request_response::cbor::Behaviour<
		proto::request_response::Request,
		proto::request_response::Response,
	>,
	stream: libp2p_stream::Behaviour,
	gossipsub: libp2p::gossipsub::Behaviour,
}

struct Node {
	state: Arc<SharedState>,
	swarm: Swarm<NodeBehaviour>,
	heartbeat_topic: gossipsub::Topic,
	response_tx_map: HashMap<
		libp2p::request_response::OutboundRequestId,
		tokio::sync::oneshot::Sender<reqres::InboundResponse>,
	>,
}

pub async fn run(
	state: Arc<SharedState>,
	mut reqres_request_rx: tokio::sync::mpsc::Receiver<
		reqres::OutboundRequestEnvelope,
	>,
	mut stream_request_rx: tokio::sync::mpsc::Receiver<
		stream::OutboundStreamRequest,
	>,
) -> eyre::Result<()> {
	let keypair = state.p2p.lock().await.keypair.clone();

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
		.with_relay_client(noise::Config::new, yamux::Config::default)?
		.with_behaviour(|keypair, relay_behaviour| {
			// To content-address message, we can take
			// the hash of message and use it as an ID.
			let message_id_fn = |message: &libp2p::gossipsub::Message| {
				let mut s = DefaultHasher::new();
				message.data.hash(&mut s);
				libp2p::gossipsub::MessageId::from(s.finish().to_string())
			};

			let gossipsub_config = libp2p::gossipsub::ConfigBuilder::default()
				// This is set to aid debugging by not cluttering the log space.
				.heartbeat_interval(Duration::from_secs(10))
				// This sets the kind of message validation.
				// The default is Strict (enforce message signing).
				.validation_mode(libp2p::gossipsub::ValidationMode::Strict)
				.message_id_fn(message_id_fn)
				.build()?;

			let gossipsub = libp2p::gossipsub::Behaviour::new(
				libp2p::gossipsub::MessageAuthenticity::Signed(keypair.clone()),
				gossipsub_config,
			)?;

			Ok(NodeBehaviour {
				mdns,
				ping: ping::Behaviour::new(ping::Config::new()),
				identify: identify::Behaviour::new(identify::Config::new(
					IDENTIFY_PROTOCOL.to_string(),
					keypair.public(),
				)),
				relay_client: relay_behaviour,
				dcutr: dcutr::Behaviour::new(keypair.public().to_peer_id()),
				request_response: libp2p::request_response::cbor::Behaviour::<
					proto::request_response::Request,
					proto::request_response::Response,
				>::new(
					[(
						StreamProtocol::new(REQUEST_RESPONSE_PROTOCOL),
						libp2p::request_response::ProtocolSupport::Full,
					)],
					libp2p::request_response::Config::default(),
				),
				stream: libp2p_stream::Behaviour::new(),
				gossipsub,
			})
		})?
		.build();

	swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;
	swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

	for addr in &state.config.bootstrap_multiaddrs {
		swarm.dial(addr.clone()).unwrap();
	}

	let mut provider_heartbeat_interval =
		tokio::time::interval(std::time::Duration::from_secs(30));

	provider_heartbeat_interval
		.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

	let heartbeat_topic = libp2p::gossipsub::IdentTopic::new("heartbeat");

	swarm
		.behaviour_mut()
		.gossipsub
		.subscribe(&heartbeat_topic)?;

	log::info!("📡 Running w/ PeerID {}", swarm.local_peer_id());

	let mut control = swarm.behaviour().stream.new_control();
	let mut incoming_streams = control
		.accept(libp2p::StreamProtocol::new(STREAM_PROTOCOL))
		.unwrap();
	drop(control);

	tokio::spawn(job_confirmation_loop(state.clone()));

	let mut node = Node {
		state,
		swarm,
		heartbeat_topic: gossipsub::Topic::Ident(heartbeat_topic),
		response_tx_map: HashMap::new(),
	};

	loop {
		#[rustfmt::skip]
		tokio::select! {
		  event = node.swarm.next() => {
        if let Some(event) = event {
          node.handle_event(event).await;
        } else {
          log::debug!("Empty swarm event, breaking loop");
          break;
        }
		  }

			envelope = reqres_request_rx.recv() => {
				if let Some(envelope) = envelope {
					let outbound_request_id = node.swarm
						.behaviour_mut()
						.request_response
						.send_request(&envelope.target_peer_id, envelope.request);

					node.response_tx_map.insert(outbound_request_id, envelope.response_tx);
				} else {
					log::warn!("reqres_request_rx closed, breaking loop");
          break;
				}
			}

			request = stream_request_rx.recv() => {
				if let Some(request) = request {
					let control = node.swarm.behaviour_mut().stream.new_control();
					let peer_id = *node.swarm.local_peer_id();
					tokio::spawn(Node::handle_outbound_stream_request(peer_id, control, request));
				} else {
					log::warn!("stream_header_rx closed, breaking loop");
          break;
				}
			}

			incoming_stream = incoming_streams.next() => {
				if let Some((peer_id, stream)) = incoming_stream {
					let future = Node::handle_incoming_stream(
						node.state.clone(),
						peer_id,
						*node.swarm.local_peer_id(),
						stream
					);

					tokio::spawn(future);
				} else {
					log::warn!("incoming_streams returned None, breaking loop");
          break;
				}
			}

      _ = provider_heartbeat_interval.tick() => {
        node.maybe_send_heartbeat().await?;
      }

      _ = node.state.shutdown_token.cancelled() => {
        log::info!("🛑 Shutting down...");
				break;
			}
		}
	}

	log::debug!("✅ Exited event loop");

	Ok(())
}

impl Node {
	async fn handle_event(&mut self, event: SwarmEvent<NodeBehaviourEvent>) {
		match event {
			SwarmEvent::Behaviour(event) => match event {
				NodeBehaviourEvent::Mdns(event) => match event {
					mdns::Event::Discovered(items) => {
						let p2p = self.state.p2p.lock().await;
						let mut peers = p2p.peers.write().await;

						for (peer_id, address) in items {
							log::debug!(
								"👀 New mDNS address discovered: {} {}",
								peer_id,
								address
							);

							// Add the address to discovered peers.
							match peers.get_mut(&peer_id) {
								Some(x) => x,
								None => {
									let set = HashSet::new();
									peers.insert(peer_id, set);
									peers.get_mut(&peer_id).unwrap()
								}
							}
							.insert(address);
						}

						p2p.peers_notify.notify_waiters();
					}

					mdns::Event::Expired(items) => {
						let p2p = self.state.p2p.lock().await;
						let mut peers = p2p.peers.write().await;

						for (peer_id, address) in items {
							log::debug!("💩 mDNS address expired: {} {}", peer_id, address);

							// Remove the address from discovered peers.
							let set = peers.get_mut(&peer_id).unwrap();
							set.remove(&address);
							if set.is_empty() {
								peers.remove(&peer_id);
							}
						}

						p2p.peers_notify.notify_waiters();
					}
				},

				NodeBehaviourEvent::Ping(event) => {
					log::trace!("{:?}", event)
				}

				NodeBehaviourEvent::RequestResponse(event) => {
					self.handle_reqres_event(event).await;
				}

				NodeBehaviourEvent::Stream(_) => todo!(),

				NodeBehaviourEvent::Gossipsub(event) => {
					self.handle_gossipsub_event(event).await;
				}

				NodeBehaviourEvent::Identify(event) => match event {
					identify::Event::Received { .. } => {
						log::debug!("🆔 [Identify] {event:?}")
					}
					identify::Event::Sent { .. } => {
						log::debug!("🆔 [Identify] {event:?}");
					}
					identify::Event::Pushed { .. } => {
						log::debug!("🆔 [Identify] {event:?}")
					}
					identify::Event::Error { .. } => {
						log::warn!("🆔 [Identify] {event:?}")
					}
				},

				NodeBehaviourEvent::RelayClient(event) => match event {
					relay::client::Event::ReservationReqAccepted { .. } => {
						log::debug!("🎏 [Relay] {event:?}")
					}

					relay::client::Event::OutboundCircuitEstablished { .. } => {
						log::debug!("🎏 [Relay] {event:?}")
					}

					relay::client::Event::InboundCircuitEstablished { .. } => {
						log::debug!("🎏 [Relay] {event:?}")
					}
				},

				NodeBehaviourEvent::Dcutr(_) => log::debug!("🥊 [DCUtR] {event:?}"),
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

			SwarmEvent::ListenerClosed { .. } => log::debug!("{event:?}"),
			SwarmEvent::ListenerError { .. } => log::warn!("{event:?}"),

			SwarmEvent::Dialing { .. } => {
				log::trace!("{:?}", event)
			}

			SwarmEvent::NewExternalAddrCandidate { .. } => log::debug!("{event:?}"),
			SwarmEvent::ExternalAddrConfirmed { .. } => log::debug!("{event:?}"),
			SwarmEvent::ExternalAddrExpired { .. } => log::debug!("{event:?}"),

			SwarmEvent::NewExternalAddrOfPeer { peer_id, .. } => {
				log::trace!("{:?}", event);
				self
					.swarm
					.behaviour_mut()
					.gossipsub
					.add_explicit_peer(&peer_id);
			}

			_ => {
				log::error!("Unhandled {:?}", event)
			}
		}
	}
}
