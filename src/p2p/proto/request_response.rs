use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "tag")]
pub enum Request {}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "tag")]
pub enum Response {}
