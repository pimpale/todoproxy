use super::AppData;
use actix_web::{http::StatusCode, web, HttpResponse, Responder, ResponseError};
use derive_more::Display;
use serde::{Deserialize, Serialize};
use todoproxy_api::response::Info;

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

pub fn report_postgres_err(e: tokio_postgres::Error) -> AppError {
    log::error!(target:"todoproxy:postgres", "{}", e);
    AppError::InternalServerError
}

pub fn report_pool_err(e: deadpool_postgres::PoolError) -> AppError {
    log::error!(target:"todoproxy:deadpool", "{}", e);
    AppError::InternalServerError
}

// respond with info about stuff
#[actix_web::post("/info")]
pub async fn info() -> Result<impl Responder, AppError> {
    return Ok(web::Json(Info {
        service: String::from(super::SERVICE),
        version_major: super::VERSION_MAJOR,
        version_minor: super::VERSION_MINOR,
        version_rev: super::VERSION_REV,
    }));
}

// start websocket connection
#[actix_web::get("/ws")]
pub async fn ws() -> Result<impl Responder, AppError> {
    return Ok(web::Json(Info {
        service: String::from(super::SERVICE),
        version_major: super::VERSION_MAJOR,
        version_minor: super::VERSION_MINOR,
        version_rev: super::VERSION_REV,
    }));
}
