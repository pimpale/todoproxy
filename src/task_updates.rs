use actix_web::web;
use auth_service_api::response::User;
use futures_util::{stream_select, StreamExt};

use actix_ws::{CloseCode, CloseReason, Message, ProtocolError};
use std::time::{Duration, Instant};
use todoproxy_api::{
    request::{WebsocketClientInitMessage, WebsocketClientOpMessage},
    response::WebsocketServerUpdateMessage,
};
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream, IntervalStream};

use crate::{handlers::AppError, AppData};

/// How often heartbeat pings are sent.
///
/// Should be half (or less) of the acceptable client timeout.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// How long before lack of client response causes a timeout.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

pub enum ConnectionState {
    Unauthenticated,
    Authenticated { user: User },
}

/// Echo text & binary messages received from the client, respond to ping messages, and monitor
/// connection health to detect network issues and free up resources.
pub async fn manage_updates_ws(
    data: web::Data<AppData>,
    mut session: actix_ws::Session,
    msg_stream: actix_ws::MessageStream,
) {
    log::info!("connected");

    let mut last_heartbeat = Instant::now();
    let mut state = ConnectionState::Unauthenticated;

    enum TaskUpdateKind {
        // we need to send a heartbeat
        NeedToSendHeartbeat,
        // we received a message from the client
        ClientMessage(Result<Message, ProtocolError>),
        // we have to handle a broadcast from the server
        ServerUpdate(Result<WebsocketServerUpdateMessage, BroadcastStreamRecvError>),
    }

    let heartbeat_stream = IntervalStream::new(tokio::time::interval(HEARTBEAT_INTERVAL))
        .map(|_| TaskUpdateKind::NeedToSendHeartbeat);
    let client_message_stream = msg_stream.map(|x| TaskUpdateKind::ClientMessage(x));
    let server_update_stream = BroadcastStream::new(data.task_update_tx.subscribe())
        .map(|x| TaskUpdateKind::ServerUpdate(x));

    let mut joint_stream = stream_select!(
        heartbeat_stream,
        client_message_stream,
        server_update_stream
    );

    let reason = loop {
        match joint_stream.next().await.unwrap() {
            // received message from WebSocket client
            TaskUpdateKind::ClientMessage(Ok(msg)) => {
                log::debug!("msg: {msg:?}");

                match msg {
                    Message::Text(text) => {
                        if let Err(e) = handle_ws_msg(data.clone(), &mut state, &text).await {
                            break Some(CloseReason {
                                code: CloseCode::Error,
                                description: Some(e.to_string()),
                            });
                        }
                    }
                    Message::Binary(_) => {
                        break Some(CloseReason {
                            code: CloseCode::Unsupported,
                            description: Some(String::from("Only text supported")),
                        });
                    }
                    Message::Close(_) => break None,
                    Message::Ping(bytes) => {
                        last_heartbeat = Instant::now();
                        let _ = session.pong(&bytes).await;
                    }
                    Message::Pong(_) => {
                        last_heartbeat = Instant::now();
                    }
                    Message::Continuation(_) => {
                        break Some(CloseReason {
                            code: CloseCode::Unsupported,
                            description: Some(String::from("No support for continuation frame.")),
                        });
                    }
                    // no-op; ignore
                    Message::Nop => {}
                };
            }
            // client WebSocket stream error
            TaskUpdateKind::ClientMessage(Err(err)) => {
                log::error!("{}", err);
                break None;
            }
            // heartbeat interval ticked
            TaskUpdateKind::NeedToSendHeartbeat => {
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
            // got message from server
            TaskUpdateKind::ServerUpdate(u) => match u {
                Ok(u) => {
                    if let ConnectionState::Authenticated { ref user } = state {
                        let send_result = session.text(serde_json::to_string(&u).unwrap()).await;
                        match send_result {
                            Ok(()) => (),
                            Err(_) => break None,
                        }
                    }
                }
                Err(BroadcastStreamRecvError::Lagged(_)) => {}
            },
        }
    };

    // attempt to close connection gracefully
    let _ = session.close(reason).await;

    log::info!("disconnected");
}

pub async fn handle_ws_msg(
    data: web::Data<AppData>,
    state: &mut ConnectionState,
    msg: &str,
) -> Result<(), AppError> {
    match state {
        ConnectionState::Unauthenticated => {
            match serde_json::from_str::<WebsocketClientInitMessage>(msg) {
                Ok(req) => handle_ws_client_init(data, state, req).await,
                Err(_) => Err(AppError::DecodeError),
            }
        }
        ConnectionState::Authenticated { user } => {
            match serde_json::from_str::<WebsocketClientOpMessage>(msg) {
                Ok(req) => handle_ws_client_op(data, state, req).await,
                Err(_) => Err(AppError::DecodeError),
            }
        }
    }
}

pub async fn handle_ws_client_init(
    data: web::Data<AppData>,
    state: &mut ConnectionState,
    req: WebsocketClientInitMessage,
) -> Result<(), AppError> {
    // make sure to push an stateset message into the broadcast stream
    Ok(())
}

pub async fn handle_ws_client_op(
    data: web::Data<AppData>,
    state: &mut ConnectionState,
    req: WebsocketClientOpMessage,
) -> Result<(), AppError> {
    return Ok(());
}
