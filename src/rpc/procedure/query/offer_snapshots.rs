use serde::{Deserialize, Serialize};

use crate::{
	db::offers::query_offer_snapshots_by_rowid,
	dto::OfferSnapshot,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
};

/// Query offer snapshots by IDs.
#[derive(Deserialize, Debug)]
pub struct OfferSnapshotsQueryRequest {
	snapshot_ids: Vec<i64>,
}

#[derive(Serialize, derive_more::Debug)]
#[serde(tag = "tag", content = "content")]
pub enum OfferSnapshotsQueryResponse {
	Ok(#[debug(skip)] Vec<OfferSnapshot>),
	Length,
}

impl Connection {
	pub async fn handle_offer_snapshots_query_request(
		&self,
		request_id: u32,
		request_data: &OfferSnapshotsQueryRequest,
	) {
		let response = 'block: {
			if request_data.snapshot_ids.len() > 100 {
				break 'block OfferSnapshotsQueryResponse::Length;
			}

			OfferSnapshotsQueryResponse::Ok(query_offer_snapshots_by_rowid(
				&*self.state.database.lock().await,
				&request_data.snapshot_ids,
			))
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::QueryOfferSnapshots(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
