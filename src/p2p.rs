use std::{
	collections::HashMap,
	fs::create_dir_all,
	hash::{DefaultHasher, Hash as _, Hasher as _},
	path::Path,
	sync::Arc,
	time::Duration,
};

use either::Either::{Left, Right};
use eyre::eyre;
use futures::{AsyncWriteExt, StreamExt as _};
use handle_heartbeat::handle_heartbeat;
use libp2p::{
	PeerId, Stream, StreamProtocol, Swarm, SwarmBuilder, gossipsub,
	identity::Keypair,
	mdns, noise, ping,
	request_response::{self, OutboundRequestId},
	swarm::{self, SwarmEvent},
	tcp, yamux,
};
use libp2p_stream::{Control, OpenStreamError};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use unwrap_none::UnwrapNone;

use crate::{
	database::create_service_connection,
	server,
	state::{ProviderOutboundRequestEnvelope, SharedState},
	util::cbor::{read_cbor, write_cbor},
};

mod handle_heartbeat;
pub mod proto;

pub type InboundResponse =
	Result<proto::request_response::Response, request_response::OutboundFailure>;

/// An outbound Request-Response protocol request envelope.
pub struct OutboundReqResRequestEnvelope {
	pub target_peer_id: PeerId,
	pub request: proto::request_response::Request,
	pub response_tx: tokio::sync::oneshot::Sender<InboundResponse>,
}

/// `(our_peer_id, stream)`.
pub type OutboundStreamRequestResult =
	Result<(PeerId, Stream), OpenStreamError>;

pub struct OutboundStreamRequest {
	pub target_peer_id: PeerId,
	pub head_request: proto::stream::HeadRequest,
	pub result_tx: tokio::sync::oneshot::Sender<OutboundStreamRequestResult>,
}

const STREAM_PROTOCOL: &str = "/derouter/stream/0.1.0";
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

pub async fn run_p2p(
	state: Arc<SharedState>,
	mut reqres_request_rx: tokio::sync::mpsc::Receiver<
		OutboundReqResRequestEnvelope,
	>,
	mut stream_request_rx: tokio::sync::mpsc::Receiver<OutboundStreamRequest>,
) -> eyre::Result<()> {
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

	let mut response_tx_map = HashMap::<
		OutboundRequestId,
		tokio::sync::oneshot::Sender<InboundResponse>,
	>::new();

	let mut control = swarm.behaviour().stream.new_control();
	let mut incoming_streams = control
		.accept(StreamProtocol::new(STREAM_PROTOCOL))
		.unwrap();

	loop {
		#[rustfmt::skip]
		tokio::select! {
		  event = swarm.next() => {
        if let Some(event) = event {
          handle_event(&state, &mut swarm, event, heartbeat_topic.clone(), &mut response_tx_map).await;
        } else {
          log::debug!("Empty swarm event, breaking loop");
          break;
        }
		  }

			envelope = reqres_request_rx.recv() => {
				if let Some(envelope) = envelope {
					let outbound_request_id = swarm
						.behaviour_mut()
						.request_response
						.send_request(&envelope.target_peer_id, envelope.request);

					response_tx_map.insert(outbound_request_id, envelope.response_tx);
				} else {
					log::warn!("reqres_request_rx closed, breaking loop");
          break;
				}
			}

			request = stream_request_rx.recv() => {
				if let Some(request) = request {
					let control = swarm.behaviour_mut().stream.new_control();
					let peer_id = *swarm.local_peer_id();
					tokio::spawn(handle_outbound_stream_request(peer_id, control, request));
				} else {
					log::warn!("stream_header_rx closed, breaking loop");
          break;
				}
			}

			incoming_stream = incoming_streams.next() => {
				if let Some((peer_id, stream)) = incoming_stream {
					let future = handle_incoming_stream(
						state.clone(),
						peer_id,
						*swarm.local_peer_id(),
						stream
					);

					tokio::spawn(async move {
						if let Err(e) = future.await {
							log::warn!("{:?}", e);
						}
					});
				} else {
					log::warn!("incoming_streams returned None, breaking loop");
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

	if lock.offers.is_empty() {
		log::debug!("No provided offers, skip heartbeat");
		return Ok(());
	}

	let mut heartbeat_offers =
		HashMap::<String, HashMap<String, proto::gossipsub::ProviderOffer>>::new();

	for (protocol_id, provided_offers_by_protocol) in &lock.offers {
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

	let message = proto::gossipsub::Heartbeat {
		provider: Some(proto::gossipsub::ProviderDetails {
			name: state.config.provider.name.clone(),
			teaser: state.config.provider.teaser.clone(),
			description: state.config.provider.description.clone(),
			offers: heartbeat_offers,
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
	response_tx_map: &mut HashMap<
		OutboundRequestId,
		tokio::sync::oneshot::Sender<InboundResponse>,
	>,
) {
	match event {
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
					request_response::Event::Message {
						peer,
						connection_id,
						message,
					} => match message {
						request_response::Message::Request { .. } => todo!(),

						request_response::Message::Response {
							request_id,
							response,
						} => {
							log::debug!(
								"ReqRes Response {{
									peer: {peer},
									connection_id: {connection_id},
									request_id: {request_id},
									response: {:?} }}",
								response
							);

							let tx = response_tx_map
								.remove(&request_id)
								.expect("should have response receiver in the map");

							let _ = tx.send(InboundResponse::Ok(response));
						}
					},

					request_response::Event::OutboundFailure {
						peer,
						connection_id,
						request_id,
						error,
					} => {
						log::warn!(
							"ReqRes OutboundFailure {{ \
								peer: {peer}, \
								connection_id: {connection_id},\
								request_id: {request_id}, \
								error: {error} }}"
						);

						let tx = response_tx_map
							.remove(&request_id)
							.expect("should have response receiver in the map");

						let _ = tx.send(InboundResponse::Err(error));
					}

					request_response::Event::InboundFailure { .. } => {
						log::warn!("ReqRes {:?}", event);
					}

					request_response::Event::ResponseSent { .. } => {
						log::debug!("ReqRes {:?}", event);
					}
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
			swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
		}

		_ => {
			log::error!("Unhandled {:?}", event)
		}
	}
}

async fn handle_outbound_stream_request(
	our_peer_id: PeerId,
	mut control: Control,
	request: OutboundStreamRequest,
) {
	let result = open_outbound_stream(our_peer_id, &mut control, &request).await;
	let _ = request.result_tx.send(result);
}

async fn open_outbound_stream(
	our_peer_id: PeerId,
	control: &mut Control,
	request: &OutboundStreamRequest,
) -> Result<(PeerId, Stream), OpenStreamError> {
	let mut stream = control
		.open_stream(request.target_peer_id, StreamProtocol::new(STREAM_PROTOCOL))
		.await?;

	let header_buffer = serde_cbor::to_vec(&request.head_request).unwrap();
	let len_buffer = (header_buffer.len() as u32).to_be_bytes();

	stream
		.write_all(&len_buffer)
		.await
		.map_err(libp2p_stream::OpenStreamError::Io)?;

	stream
		.write_all(&header_buffer)
		.await
		.map_err(libp2p_stream::OpenStreamError::Io)?;

	log::debug!("Successfully written {:?}", request.head_request);

	Ok((our_peer_id, stream))
}

async fn handle_incoming_stream(
	state: Arc<SharedState>,
	from_peer_id: PeerId,
	to_peer_id: PeerId,
	stream: Stream,
) -> eyre::Result<()> {
	log::info!("🌊 Incoming stream from {:?}", from_peer_id);
	let mut stream = stream.compat();

	let header: proto::stream::HeadRequest = match read_cbor(&mut stream).await? {
		Some(header) => {
			log::debug!("Read {:?}", header);
			header
		}

		None => {
			log::warn!("P2P stream EOF'ed before header is read");
			return Ok(());
		}
	};

	match header {
		proto::stream::HeadRequest::ServiceConnection {
			protocol_id,
			offer_id,
			protocol_payload,
		} => {
			log::debug!("Locking provider...");
			let provider = state.provider.lock().await;
			let provided_offer = provider
				.offers
				.get(&protocol_id)
				.and_then(|o| o.get(&offer_id))
				.cloned();
			drop(provider);

			type ServiceConnectionHeadResponse =
				proto::stream::ServiceConnectionHeadResponse;

			let response = if let Some(ref provided_offer) = provided_offer {
				let provided_payload_string =
					serde_json::to_string(&provided_offer.protocol_payload)
						.expect("should serialize provided offer payload");

				if provided_payload_string == *protocol_payload {
					log::debug!("🤝 Authorized incoming P2P stream");
					ServiceConnectionHeadResponse::Ok
				} else {
					log::debug!(
						"Protocol payload mismatch: {} vs {}",
						provided_payload_string,
						protocol_payload
					);

					ServiceConnectionHeadResponse::OfferNotFoundError
				}
			} else {
				log::debug!("Could not find offer {} => {}", protocol_id, offer_id);
				ServiceConnectionHeadResponse::OfferNotFoundError
			};

			let head_response =
				proto::stream::HeadResponse::ServiceConnection(response.clone());

			log::debug!("{:?}", head_response);
			write_cbor(&mut stream, &head_response).await??;

			let mut provided_offer = match response {
				proto::stream::ServiceConnectionHeadResponse::Ok => {
					provided_offer.unwrap()
				}
				_ => return Ok(()),
			};

			let mut database = state.database.lock().await;

			let offer_snapshot =
				if let Some(snapshot_rowid) = provided_offer.snapshot_rowid {
					// If the provided offer is saved to DB, reuse its ROWID.
					Left(snapshot_rowid)
				} else {
					// Otherwise, insert a new offer snapshot.
					Right((
						to_peer_id,
						&offer_id,
						&protocol_id,
						&provided_offer.protocol_payload,
					))
				};

			let (offer_snapshot_rowid, connection_rowid) =
				create_service_connection(&mut database, offer_snapshot, from_peer_id);

			drop(database);

			// Mark the snapshot as saved in DB.
			provided_offer.snapshot_rowid = Some(offer_snapshot_rowid);

			let mut provider = state.provider.lock().await;

			let module = match provider.modules.get_mut(&provided_offer.provider_id) {
				Some(x) => x,
				None => {
					return Err(eyre!(
						"Provider module \"{}\" not found",
						provided_offer.provider_id
					));
				}
			};

			type OutboundRequestFrameData =
				server::provider::rpc::OutboundRequestFrameData;

			let (response_tx, response_rx) = tokio::sync::oneshot::channel();

			let envelope = ProviderOutboundRequestEnvelope {
				frame_data: OutboundRequestFrameData::OpenConnection {
					customer_peer_id: from_peer_id.to_base58(),
					protocol_id,
					offer_id,
					protocol_payload: provided_offer.protocol_payload,
					connection_id: connection_rowid,
				},
				response_tx,
			};

			module.outbound_request_tx.try_send(envelope).map_err(|e| {
				eyre!(
					"per_module_outbound_request_txs[\"{}\"].send failed: {:?}",
					provided_offer.provider_id,
					e
				)
			})?;

			module
				.future_service_connections
				.lock()
				.await
				.insert(connection_rowid, stream.into_inner());

			drop(provider);

			log::debug!(
				"Provider \"{}\" service connection {} is waiting for Yamux stream",
				provided_offer.provider_id,
				connection_rowid
			);

			match response_rx.await.map_err(|e| eyre!(e))? {
				server::provider::rpc::InboundResponseFrameData::Ack => {}
			}

			Ok(())
		}
	}
}
