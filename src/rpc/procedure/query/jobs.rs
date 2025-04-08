use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{
	db::service_jobs::query_all::query_jobs,
	dto::JobRecord,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
};

/// Query jobs in the ascending order of creation.
#[derive(Deserialize, Debug)]
pub struct JobsQueryRequest {
	/// If set, would return jobs greater than (excluding).
	database_row_id_cursor: Option<i64>,

	/// If set, would filter jobs by their offers' protocol ID.
	protocol_ids: Option<Vec<String>>,

	/// If set, would filter jobs by their provider peer ID.
	provider_peer_ids: Option<Vec<String>>,

	/// If set, would filter jobs by their consumer peer ID.
	consumer_peer_ids: Option<Vec<String>>,

	/// Limit the number of results.
	limit: i64,
}

#[derive(Serialize, derive_more::Debug)]
#[serde(tag = "tag", content = "content")]
pub enum JobsQueryResponse {
	Ok(#[debug(skip)] Vec<JobRecord>),
	InvalidPeerId(String),
	InvalidLimit,
}

impl Connection {
	pub async fn handle_jobs_query_request(
		&self,
		request_id: u32,
		request_data: &JobsQueryRequest,
	) {
		let response = 'block: {
			if request_data.limit < 1 || request_data.limit > 100 {
				break 'block JobsQueryResponse::InvalidLimit;
			}

			let provider_peer_ids =
				if let Some(raw_peer_ids) = &request_data.provider_peer_ids {
					let mut parsed_peer_ids = vec![];

					for raw_peer_id in raw_peer_ids {
						parsed_peer_ids.push(match libp2p::PeerId::from_str(raw_peer_id) {
							Ok(x) => x,
							Err(_) => {
								break 'block JobsQueryResponse::InvalidPeerId(
									raw_peer_id.to_string(),
								);
							}
						});
					}

					Some(parsed_peer_ids)
				} else {
					None
				};

			let consumer_peer_ids =
				if let Some(raw_peer_ids) = &request_data.consumer_peer_ids {
					let mut parsed_peer_ids = vec![];

					for raw_peer_id in raw_peer_ids {
						parsed_peer_ids.push(match libp2p::PeerId::from_str(raw_peer_id) {
							Ok(x) => x,
							Err(_) => {
								break 'block JobsQueryResponse::InvalidPeerId(
									raw_peer_id.to_string(),
								);
							}
						});
					}

					Some(parsed_peer_ids)
				} else {
					None
				};

			JobsQueryResponse::Ok(query_jobs(
				&mut *self.state.db.lock().await,
				request_data.database_row_id_cursor,
				request_data.protocol_ids.as_deref(),
				provider_peer_ids.as_deref(),
				consumer_peer_ids.as_deref(),
				request_data.limit,
			))
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::QueryJobs(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
