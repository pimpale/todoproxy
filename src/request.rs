use serde::{Deserialize, Serialize};

use super::db_types::TaskStatus;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveTaskNewCreateProps {
    pub api_key: String,
    pub value: String,
    pub position: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveTaskNewRestoreProps {
    pub api_key: String,
    pub finished_task_id: i64,
    pub position: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveTaskEditProps {
    pub api_key: String,
    pub live_task_id: i64,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveTaskDeleteProps {
    pub api_key: String,
    pub live_task_id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveTaskRotateUpProps {
    pub api_key: String,
    pub live_task_id_src: i64,
    pub live_task_id_dst: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FinishedTaskNewProps {
    pub api_key: String,
    pub live_task_id: i64,
    pub status: TaskStatus,
}
