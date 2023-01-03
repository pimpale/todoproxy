use actix_web::web;
use auth_service_api::response::User;
use futures_util::{stream_select, StreamExt};

use actix_ws::{CloseCode, CloseReason, Message, ProtocolError};
use std::time::{Duration, Instant};
use todoproxy_api::{
    request::{WebsocketClientInitMessage, WebsocketClientOpMessage},
    response::ServerStateCheckpoint,
};
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream, IntervalStream};

use crate::db_types;
use crate::handlers::{self, get_user_if_api_key_valid};
use crate::{handlers::AppError, AppData};

/// How often heartbeat pings are sent.
///
/// Should be half (or less) of the acceptable client timeout.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// How long before lack of client response causes a timeout.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

struct ConnectionState {
    user: User,
}

pub async fn manage_updates_ws(
    data: web::Data<AppData>,
    mut session: actix_ws::Session,
    msg_stream: actix_ws::MessageStream,
) {
    log::info!("connected");

    let mut last_heartbeat = Instant::now();

    enum TaskUpdateKind {
        // we need to send a heartbeat
        NeedToSendHeartbeat,
        // we received a message from the client
        ClientMessage(Result<Message, ProtocolError>),
        // we have to handle a broadcast from the server
        ServerUpdate(Result<db_types::Operation, BroadcastStreamRecvError>),
    }

    let heartbeat_stream = IntervalStream::new(tokio::time::interval(HEARTBEAT_INTERVAL))
        .map(|_| TaskUpdateKind::NeedToSendHeartbeat);
    let client_message_stream = msg_stream.map(|x| TaskUpdateKind::ClientMessage(x));

    let mut joint_stream = stream_select!(heartbeat_stream, client_message_stream,);

    let reason = loop {
        match joint_stream.next().await.unwrap() {
            // received message from WebSocket client
            TaskUpdateKind::ClientMessage(Ok(msg)) => {
                log::debug!("msg: {msg:?}");
                match msg {
                    Message::Text(text) => {
                        // try to parse the json
                        break serde_json::from_str::<WebsocketClientInitMessage>(&text).map_err(
                            |e| {
                                Some(CloseReason {
                                    code: CloseCode::Error,
                                    description: Some(e.to_string()),
                                })
                            },
                        );
                    }
                    Message::Binary(_) => {
                        break Err(Some(CloseReason {
                            code: CloseCode::Unsupported,
                            description: Some(String::from("Only text supported")),
                        }));
                    }
                    Message::Close(_) => break Err(None),
                    Message::Ping(bytes) => {
                        last_heartbeat = Instant::now();
                        let _ = session.pong(&bytes).await;
                    }
                    Message::Pong(_) => {
                        last_heartbeat = Instant::now();
                    }
                    Message::Continuation(_) => {
                        break Err(Some(CloseReason {
                            code: CloseCode::Unsupported,
                            description: Some(String::from("No support for continuation frame.")),
                        }));
                    }
                    // no-op; ignore
                    Message::Nop => {}
                };
            }
            // client WebSocket stream error
            TaskUpdateKind::ClientMessage(Err(err)) => {
                log::error!("{}", err);
                break Err(None);
            }
            // heartbeat interval ticked
            TaskUpdateKind::NeedToSendHeartbeat => {
                // if no heartbeat ping/pong received recently, close the connection
                if Instant::now().duration_since(last_heartbeat) > CLIENT_TIMEOUT {
                    log::info!("client has not sent heartbeat in over {CLIENT_TIMEOUT:?}");
                    break Err(None);
                }
                // send heartbeat ping
                let _ = session.ping(b"").await;
            }
            // got message from server (impossible)
            TaskUpdateKind::ServerUpdate(u) => {}
        }
    };

    // get request and otherwise disconnect
    let init_msg = match reason {
        Err(reason) => {
            // attempt to close connection gracefully
            let _ = session.close(reason).await;
            log::info!("disconnected init");
            return;
        }
        Ok(req) => req,
    };

    // try block for app
    let reason = try {
        log::info!("trying to get user");
        let user = get_user_if_api_key_valid(&data.auth_service, init_msg.api_key).await?;

    };

    let server_update_stream = BroadcastStream::new(data.task_update_tx.subscribe())
        .map(|x| TaskUpdateKind::ServerUpdate(x));

    let mut joint_stream = stream_select!(joint_stream, server_update_stream);

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
                Ok(op) => {
                    if let ConnectionState::Authenticated(AuthenticatedConnectionState {
                        ref user,
                    }) = state
                    {
                        if user.user_id == op.creator_user_id {
                            let send_result = session.text(op.jsonval).await;
                            match send_result {
                                Ok(()) => (),
                                Err(_) => break None,
                            }
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
}

pub async fn handle_ws_client_init(
    data: web::Data<AppData>,
    state: &mut ConnectionState,
    req: WebsocketClientInitMessage,
) -> Result<(), AppError> {
    // try to initialize the rwlock

    // initialize authentication state
    *state = ConnectionState::Authenticated {
        user,
        snapshot: ServerStateCheckpoint {
            live: vec![],
            finished: vec![],
        },
    };

    // make sure to push an stateset message into the broadcast stream
    Ok(())
}

pub async fn handle_ws_client_op(
    data: web::Data<AppData>,
    state: &mut ConnectionState,
    req: WebsocketClientOpMessage,
) -> Result<(), AppError> {
    let (user, snapshot) = if let ConnectionState::Authenticated { user, snapshot } = state {
        (user, snapshot)
    } else {
        return Err(AppError::Unauthorized);
    };

    // submit it to the writer thread for this user
    // make modification to the current snapshot
    match req {
        WebsocketClientOpMessage::LiveTaskInsNew { value, position } => todo!(),
        WebsocketClientOpMessage::LiveTaskInsRestore { finished_task_id } => todo!(),
        WebsocketClientOpMessage::LiveTaskEdit {
            live_task_id,
            value,
        } => todo!(),
        WebsocketClientOpMessage::LiveTaskDel { live_task_id } => todo!(),
        WebsocketClientOpMessage::LiveTaskDelIns {
            live_task_id_del,
            live_task_id_ins,
        } => todo!(),
        WebsocketClientOpMessage::FinishedTaskNew {
            live_task_id,
            status,
        } => todo!(),
    }

    return Ok(());
}

// in case there is more than one websocket connecting to the same server, we need a way to order sql reads and writes to the same user
// to do this, we'll initialize a worker that is responsible for writing to sql and updating the rw lock
// how do we know wrt
pub async fn user_worker() {}
