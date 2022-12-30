use todoproxy_api::TaskStatus;

#[derive(Clone, Debug)]
pub struct LiveTask {
    pub live_task_id: i64,
    pub creation_time: i64,
    pub creator_user_id: i64,
    pub position: i64,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct FinishedTask {
    pub finished_task_id: i64,
    pub creation_time: i64,
    pub creator_user_id: i64,
    pub value: String,
    pub status: TaskStatus,
}
