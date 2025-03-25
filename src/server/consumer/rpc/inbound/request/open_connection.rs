use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct OpenConnection {
	pub provider_peer_id: libp2p::PeerId,
	pub offer_id: String,
	pub protocol_id: String,
}
