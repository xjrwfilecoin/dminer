use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum PollingState {
    Started(u64),
    Pending,
    Done(Value),
    Removed,
    Error(PollingError),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum PollingError {
    NotExist,
    Disconnected,
}
