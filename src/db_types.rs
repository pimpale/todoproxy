use serde::{Deserialize, Serialize};
use derive_more::Display;

#[derive(Clone, Debug, Serialize, Deserialize, Display)]
pub enum TaskStatus {
    Obsoleted,
    Succeeded,
    Failed
}

#[derive(Clone, Debug)]
pub struct LiveTask {
    pub live_task_id: i64,
    pub creation_time: i64,
    pub creator_user_id: i64,
    pub position: i64,
    pub value: String
}

#[derive(Clone, Debug)]
pub struct FinishedTask {
    pub finished_task_id: i64,
    pub creation_time: i64,
    pub creator_user_id: i64,
    pub value: String,
    pub status: TaskStatus,
}
