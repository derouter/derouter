use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct OpenConnection {
	pub offer_snapshot_id: i64,
}
