use serde::{Deserialize, Serialize};

use crate::database::service_connections::Currency;

/// Written immediately to an outbound stream.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum HeadRequest {
	ServiceConnection {
		protocol_id: String,
		offer_id: String,
		protocol_payload: String,
		currency: Currency,
	},
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ServiceConnectionHeadResponse {
	Ok,
	OfferNotFoundError,
	AnotherError(String),
}

/// Written to the stream in response to [HeadRequest].
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum HeadResponse {
	ServiceConnection(ServiceConnectionHeadResponse),
}
