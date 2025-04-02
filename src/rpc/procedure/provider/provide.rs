use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use unwrap_none::UnwrapNone;

use crate::{
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
	state::{ProvidedOffer, ProviderModuleState},
};

/// Provide an offer until the RPC connection shuts down.
#[derive(Deserialize, Debug)]
pub struct ProviderProvideRequest {
	protocol_id: String,
	offer_id: String,
	protocol_payload: serde_json::Value,
}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ProviderProvideResponse {
	Ok,

	/// The backend is already providing an offer with the same protocol
	/// and offer IDs (may or may not be from the same module).
	DuplicateOffer {
		protocol_id: String,
		offer_id: String,
	},
}

impl Connection {
	pub async fn handle_provider_provide_request(
		&mut self,
		request_id: u32,
		request_data: &ProviderProvideRequest,
	) {
		let response = 'block: {
			let mut provider_lock = self.state.provider.lock().await;

			let protocol_id = &request_data.protocol_id;
			let offer_id = &request_data.offer_id;

			// Look for existing offer.
			//

			if let Some(modules_by_protocol) =
				provider_lock.offers_module_map.get(protocol_id)
			{
				if modules_by_protocol.get(offer_id).is_some() {
					log::warn!("Duplicate offer ({} => {})", protocol_id, offer_id);

					break 'block ProviderProvideResponse::DuplicateOffer {
						protocol_id: protocol_id.clone(),
						offer_id: offer_id.clone(),
					};
				}
			}

			// Find or create a new provider state module.
			//

			let module = if let Some(module_id) = &self.provider_module_id {
				provider_lock.modules.get_mut(module_id).unwrap()
			} else {
				let module = ProviderModuleState {
					outbound_request_tx: self.outbound_request_tx.clone(),
					future_service_connections: self.future_connections.clone(),
					offers: HashMap::new(),
				};

				let module_id = provider_lock.module_ids_counter;
				self.provider_module_id = Some(module_id);
				provider_lock.module_ids_counter += 1;

				provider_lock
					.modules
					.insert(module_id, module)
					.unwrap_none();

				provider_lock.modules.get_mut(&module_id).unwrap()
			};

			// Insert the offer into the module's `offers` map.
			//

			let module_offers_by_id = match module.offers.get_mut(protocol_id) {
				Some(x) => x,
				None => &mut {
					module.offers.insert(protocol_id.clone(), HashMap::new());
					module.offers.get_mut(protocol_id).unwrap()
				},
			};

			module_offers_by_id
				.insert(
					request_data.offer_id.clone(),
					ProvidedOffer {
						snapshot_rowid: None,
						protocol_payload: request_data.protocol_payload.clone(),
					},
				)
				.unwrap_none();

			// Insert the module's ID into the provider's `offers_module_map`.
			//

			let offers_module_map =
				match provider_lock.offers_module_map.get_mut(protocol_id) {
					Some(x) => x,
					None => {
						provider_lock
							.offers_module_map
							.insert(protocol_id.clone(), HashMap::new());
						provider_lock
							.offers_module_map
							.get_mut(protocol_id)
							.unwrap()
					}
				};

			offers_module_map
				.insert(offer_id.clone(), self.provider_module_id.unwrap());

			provider_lock.last_updated_at = chrono::Utc::now();

			// Done!
			//

			log::trace!("Providing {:?}", request_data);

			ProviderProvideResponse::Ok
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::ProviderProvide(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
