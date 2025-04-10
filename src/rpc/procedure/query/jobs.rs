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
	provider_peer_ids: Option<Vec<libp2p::PeerId>>,

	/// If set, would filter jobs by their consumer peer ID.
	consumer_peer_ids: Option<Vec<libp2p::PeerId>>,

	/// Limit the number of results.
	limit: i64,
}

#[derive(Serialize, derive_more::Debug)]
#[serde(tag = "tag", content = "content")]
pub enum JobsQueryResponse {
	Ok(#[debug(skip)] Vec<JobRecord>),
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

			JobsQueryResponse::Ok(query_jobs(
				&mut *self.state.db.lock().await,
				request_data.database_row_id_cursor,
				request_data.protocol_ids.as_deref(),
				request_data.provider_peer_ids.as_deref(),
				request_data.consumer_peer_ids.as_deref(),
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
