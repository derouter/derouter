use libp2p::{PeerId, request_response::ResponseChannel};

use crate::{
	db::{self, service_jobs::get::find_by_job_id},
	p2p::{
		Node,
		proto::{
			self,
			request_response::{GetJobRequest, GetJobResponse},
		},
	},
};

impl Node {
	pub(super) async fn handle_get_job_request(
		&mut self,
		from_peer_id: PeerId,
		request: GetJobRequest,
		response_channel: ResponseChannel<proto::request_response::Response>,
	) {
		let conn = self.state.db.lock().await;
		let our_peer_id = *self.swarm.local_peer_id();

		let response = 'a: {
			let job = match find_by_job_id(
				db::ConnectionLike::Connection(&conn),
				our_peer_id,
				&request.provider_job_id,
			) {
				Some(job) => job,
				None => break 'a GetJobResponse::JobNotFound,
			};

			if from_peer_id == job.consumer_peer_id {
				GetJobResponse::Ok {
					public_payload: job.public_payload,
					balance_delta: job.balance_delta,
					created_at_sync: job.created_at_sync,
					completed_at_sync: job.completed_at_sync,
				}
			} else {
				GetJobResponse::JobNotFound
			}
		};

		let response = proto::request_response::Response::GetJob(response);
		log::debug!("Sending {:?}..", response);

		// TODO: Wait for outbound failures.
		match self
			.swarm
			.behaviour_mut()
			.request_response
			.send_response(response_channel, response)
		{
			Ok(_) => {}
			Err(response) => {
				log::warn!("Failed to send {:?}", response)
			}
		}
	}
}
