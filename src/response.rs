use serde::{Deserialize, Serialize};

use derive_more::Display;

use super::db_types::TaskStatus;
use actix_web::http::StatusCode;
use actix_web::{error::ResponseError, HttpResponse};

#[derive(Clone, Debug, Serialize, Deserialize, Display)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppError {
    DecodeError,
    InternalServerError,
    Unauthorized,
    BadRequest,
    NotFound,
    InvalidBase64,
    Unknown,
}

impl ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(self)
    }
    fn status_code(&self) -> StatusCode {
        match *self {
            AppError::DecodeError => StatusCode::BAD_GATEWAY,
            AppError::InternalServerError => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::BadRequest => StatusCode::BAD_REQUEST,
            AppError::InvalidBase64 => StatusCode::BAD_REQUEST,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::Unknown => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTask {
    pub task_id: i64,
    pub creation_time: i64,
    pub creator_user_id: i64,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishedTask {
    pub finished_task_id: i64,
    pub creation_time: i64,
    pub creator_user_id: i64,
    pub value: String,
    pub status: TaskStatus,
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveTaskNewCreateUpdate {
    pub value: String,
    pub position: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveTaskNewRestoreUpdate {
    pub finished_task_id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveTaskEditUpdate {
    pub live_task_id: i64,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveTaskDeleteUpdate {
    pub live_task_id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveTaskDelInsertUpdate {
    pub live_task_id_del: i64,
    pub live_task_id_ins: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FinishedTaskNewUpdate {
    pub live_task_id: i64,
    pub status: TaskStatus,
}

pub enum LiveTaskUpdate {
    Delete(LiveTaskDel),
    Update(String),
    Insert(i64, LiveTask),


}
