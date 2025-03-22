use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct ConsumerConfig {
	pub protocols: Vec<String>,
}
