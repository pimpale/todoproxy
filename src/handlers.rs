use std::time::{Duration, Instant};

use super::AppData;
use actix_web::{
    http::StatusCode, rt, web, Error, HttpRequest, HttpResponse, Responder, ResponseError,
};
use actix_ws::Message;
use derive_more::Display;
use futures_util::{
    future::{self, Either},
    StreamExt,
};
use serde::{Deserialize, Serialize};
use todoproxy_api::response::Info;
use tokio::time::interval;

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
    log::error!("{}", e);
    AppError::InternalServerError
}

pub fn report_pool_err(e: deadpool_postgres::PoolError) -> AppError {
    log::error!("{}", e);
    AppError::InternalServerError
}

// respond with info about stuff
pub async fn info() -> Result<impl Responder, AppError> {
    return Ok(web::Json(Info {
        service: String::from(super::SERVICE),
        version_major: super::VERSION_MAJOR,
        version_minor: super::VERSION_MINOR,
        version_rev: super::VERSION_REV,
    }));
}

// start websocket connection
pub async fn ws(
    data: web::Data<AppData>,
    req: HttpRequest,
    stream: web::Payload,
) -> Result<impl Responder, Error> {
    let (res, session, msg_stream) = actix_ws::handle(&req, stream)?;
    // spawn websocket handler (and don't await it) so that the response is returned immediately
    rt::spawn(manage_updates_ws(data, session, msg_stream));
    Ok(res)
}

/// How often heartbeat pings are sent.
///
/// Should be half (or less) of the acceptable client timeout.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// How long before lack of client response causes a timeout.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

pub enum ConnectionState {
    Unauthenticated,
    Authenticated {
    }
}

/// Echo text & binary messages received from the client, respond to ping messages, and monitor
/// connection health to detect network issues and free up resources.
pub async fn manage_updates_ws(
    data: web::Data<AppData>,
    mut session: actix_ws::Session,
    mut msg_stream: actix_ws::MessageStream,
) {
    log::info!("connected");

    let mut last_heartbeat = Instant::now();
    let mut interval = interval(HEARTBEAT_INTERVAL);

    let reason = loop {
        // create "next client timeout check" future
        let tick = interval.tick();
        // required for select()
        tokio::pin!(tick);

        // waits for either `msg_stream` to receive a message from the client or the heartbeat
        // interval timer to tick, yielding the value of whichever one is ready first
        match future::select(msg_stream.next(), tick).await {
            // received message from WebSocket client
            Either::Left((Some(Ok(msg)), _)) => {
                log::debug!("msg: {msg:?}");

                match msg {
                    Message::Text(text) => {
                        session.text(text).await.unwrap();
                    }

                    Message::Binary(bin) => {
                        session.binary(bin).await.unwrap();
                    }

                    Message::Close(reason) => {
                        break reason;
                    }

                    Message::Ping(bytes) => {
                        last_heartbeat = Instant::now();
                        let _ = session.pong(&bytes).await;
                    }

                    Message::Pong(_) => {
                        last_heartbeat = Instant::now();
                    }

                    Message::Continuation(_) => {
                        log::warn!("no support for continuation frames");
                    }
                    // no-op; ignore
                    Message::Nop => {}
                };
            }

            // client WebSocket stream error
            Either::Left((Some(Err(err)), _)) => {
                log::error!("{}", err);
                break None;
            }

            // client WebSocket stream ended
            Either::Left((None, _)) => break None,

            // heartbeat interval ticked
            Either::Right((_inst, _)) => {
                // if no heartbeat ping/pong received recently, close the connection
                if Instant::now().duration_since(last_heartbeat) > CLIENT_TIMEOUT {
                    log::info!(
                        "client has not sent heartbeat in over {CLIENT_TIMEOUT:?}; disconnecting"
                    );

                    break None;
                }

                // send heartbeat ping
                let _ = session.ping(b"").await;
            }
        }
    };

    // attempt to close connection gracefully
    let _ = session.close(reason).await;

    log::info!("disconnected");
}
