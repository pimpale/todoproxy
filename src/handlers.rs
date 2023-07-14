use super::task_updates;
use super::AppData;

use actix_web::{
    http::StatusCode, rt, web, Error, HttpRequest, HttpResponse, Responder, ResponseError,
};
use auth_service_api::response::{AuthError, User};
use derive_more::Display;
use serde::{Deserialize, Serialize};

use todoproxy_api::request;
use todoproxy_api::response;

#[derive(Clone, Debug, Serialize, Deserialize, Display)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppError {
    DecodeError,
    InternalServerError,
    Unauthorized,
    BadRequest,
    NotFound,
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
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::Unknown => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

pub fn report_postgres_err(e: tokio_postgres::Error) -> AppError {
    log::error!("{}", e);
    AppError::InternalServerError
}

pub fn report_pool_err(e: deadpool_postgres::PoolError) -> AppError {
    log::error!("{}", e);
    AppError::InternalServerError
}

pub fn report_internal_serde_error(e: serde_json::Error) -> AppError {
    log::error!("{}", e);
    AppError::InternalServerError
}

pub fn report_serde_error(e: serde_json::Error) -> AppError {
    log::info!("{}", e);
    AppError::DecodeError
}

pub fn report_auth_err(e: AuthError) -> AppError {
    match e {
        AuthError::ApiKeyNonexistent => AppError::Unauthorized,
        AuthError::ApiKeyUnauthorized => AppError::Unauthorized,
        c => {
            let ae = match c {
                AuthError::InternalServerError => AppError::InternalServerError,
                AuthError::MethodNotAllowed => AppError::InternalServerError,
                AuthError::BadRequest => AppError::InternalServerError,
                AuthError::Network => AppError::InternalServerError,
                _ => AppError::Unknown,
            };
            log::error!("auth: {}", c);
            ae
        }
    }
}

pub async fn get_user_if_api_key_valid(
    auth_service: &auth_service_api::client::AuthService,
    api_key: String,
) -> Result<User, AppError> {
    auth_service
        .get_user_by_api_key_if_valid(api_key)
        .await
        .map_err(report_auth_err)
}

// respond with info about stuff
pub async fn info(data: web::Data<AppData>) -> Result<impl Responder, AppError> {
    let info = data.auth_service.info().await.map_err(report_auth_err)?;
    return Ok(web::Json(response::Info {
        service: String::from(super::SERVICE),
        version_major: super::VERSION_MAJOR,
        version_minor: super::VERSION_MINOR,
        version_rev: super::VERSION_REV,
        app_pub_origin: data.app_pub_origin.clone(),
        auth_pub_api_href: info.app_pub_api_href,
        auth_authenticator_href: info.app_authenticator_href,
    }));
}

// start websocket connection
pub async fn ws_task_updates(
    data: web::Data<AppData>,
    req: HttpRequest,
    stream: web::Payload,
    query: web::Query<request::WebsocketInitMessage>,
) -> Result<impl Responder, Error> {
    let (res, session, msg_stream) = actix_ws::handle(&req, stream)?;
    // spawn websocket handler (and don't await it) so that the response is returned immediately
    rt::spawn(task_updates::manage_updates_ws(
        data,
        query.into_inner(),
        session,
        msg_stream,
    ));
    Ok(res)
}
