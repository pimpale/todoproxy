use actix_web::web;
use auth_service_api::response::User;
use futures_util::{stream_select, StreamExt};

use actix_ws::{CloseCode, CloseReason, Message, ProtocolError};
use std::{
    collections::{hash_map::Entry, VecDeque},
    time::{Duration, Instant},
};
use todoproxy_api::{
    request::{WebsocketClientInitMessage, WebsocketClientOpMessage},
    response::{FinishedTask, LiveTask, ServerStateCheckpoint, WebsocketServerUpdateMessage},
};
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream, IntervalStream};

use crate::db_types;
use crate::handlers::{self, get_user_if_api_key_valid};
use crate::{checkpoint_service, operation_service, PerUserWorkerData};
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
    let maybe_per_user_worker_data: Result<PerUserWorkerData, AppError> = try {
        log::info!("trying to get user");
        let user = get_user_if_api_key_valid(&data.auth_service, init_msg.api_key).await?;
        log::info!("validated conenction for user {}", user.user_id);

        // initialize connection
        let con: &mut tokio_postgres::Client =
            &mut *data.pool.get().await.map_err(handlers::report_pool_err)?;

        // get recent checkpoitn
        let maybe_recent_checkpoint =
            checkpoint_service::get_recent_by_user_id(&mut *con, user.user_id)
                .await
                .map_err(handlers::report_postgres_err)?;

        // get all operations since this checkpoint (if it exists)
        let operations_since_last_checkpoint = match maybe_recent_checkpoint {
            Some(db_types::Checkpoint { checkpoint_id, .. }) => {
                operation_service::get_operations_since(&mut *con, checkpoint_id)
                    .await
                    .map_err(handlers::report_postgres_err)?
            }
            None => vec![],
        };

        let per_user_worker_data = {
            let mut write_guard = data.user_worker_data.lock().await;
            match write_guard.entry(user.user_id) {
                Entry::Vacant(v) => {
                    // create channel
                    let (updates_tx, updates_rx) = tokio::sync::broadcast::channel(1000);
                    // create snapshot from checkpoint
                    let mut snapshot = match maybe_recent_checkpoint {
                        Some(checkpoint) => {
                            serde_json::from_str::<ServerStateCheckpoint>(&checkpoint.jsonval)
                                .map_err(handlers::report_internal_serde_error)?
                        }
                        None => ServerStateCheckpoint {
                            live: VecDeque::new(),
                            finished: vec![],
                        },
                    };

                    let most_recent_operation_id = operations_since_last_checkpoint
                        .last()
                        .map(|x| x.operation_id);

                    for x in operations_since_last_checkpoint {
                        let op = serde_json::from_str::<WebsocketServerUpdateMessage>(&x.jsonval)
                            .map_err(handlers::report_internal_serde_error)?;
                        snapshot = apply_operation(snapshot, op);
                    }

                    let per_user_worker_data_ref = v.insert(PerUserWorkerData {
                        updates_tx,
                        snapshot,
                        user,
                        most_recent_operation_id,
                    });

                    // spawn tokio task to for
                    //tokio::spawn(user_worker);

                    per_user_worker_data_ref.clone()
                }
                Entry::Occupied(o) => o.get().clone(),
            }
        };
        // return per user worker data on success
        per_user_worker_data
    };

    let per_user_worker_data = match maybe_per_user_worker_data {
        Ok(v) => v,
        Err(e) => {
            // attempt to close connection gracefully
            let _ = session
                .close(Some(CloseReason {
                    code: CloseCode::Error,
                    description: Some(e.to_string()),
                }))
                .await;
            log::info!("disconnected init");
            return;
        }
    };

    let server_update_stream = BroadcastStream::new(per_user_worker_data.updates_tx.subscribe())
        .map(|x| TaskUpdateKind::ServerUpdate(x));

    let mut joint_stream = stream_select!(joint_stream, server_update_stream);

    let reason = loop {
        match joint_stream.next().await.unwrap() {
            // received message from WebSocket client
            TaskUpdateKind::ClientMessage(Ok(msg)) => {
                log::debug!("msg: {msg:?}");

                match msg {
                    Message::Text(text) => {
                        if let Err(e) =
                            handle_ws_client_op(data.clone(), &per_user_worker_data, &text).await
                        {
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
                    let send_result = session.text(op.jsonval).await;
                    match send_result {
                        Ok(()) => (),
                        Err(_) => break None,
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

pub async fn handle_ws_client_op(
    data: web::Data<AppData>,
    per_user_worker_data: &PerUserWorkerData,
    req: &str,
) -> Result<(), AppError> {
    let opreq = serde_json::from_str::<WebsocketClientOpMessage>(req)
        .map_err(handlers::report_serde_error)?;

    // submit it to the writer thread for this user
    // make modification to the current snapshot
    match opreq {
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

fn apply_operation(
    ServerStateCheckpoint {
        mut finished,
        mut live,
    }: ServerStateCheckpoint,
    op: WebsocketServerUpdateMessage,
) -> ServerStateCheckpoint {
    match op {
        WebsocketServerUpdateMessage::OverwriteState(s) => s,
        WebsocketServerUpdateMessage::LiveTaskInsNew {
            value,
            live_task_id,
            position,
        } => {
            if position <= live.len() {
                live.insert(
                    position,
                    LiveTask {
                        id: live_task_id,
                        value,
                    },
                );
            }
            ServerStateCheckpoint { live, finished }
        }
        WebsocketServerUpdateMessage::LiveTaskInsRestore { finished_task_id } => {
            // if it was found in the finished list, push it to the front
            if let Some(position) = finished.iter().position(|x| x.id == finished_task_id) {
                let FinishedTask { id, value, .. } = finished.remove(position);
                live.push_front(LiveTask { id, value });
            }
            ServerStateCheckpoint { live, finished }
        }
        WebsocketServerUpdateMessage::LiveTaskEdit {
            live_task_id,
            value,
        } => {
            for x in live.iter_mut() {
                if x.id == live_task_id {
                    x.value = value;
                    break;
                }
            }
            ServerStateCheckpoint { live, finished }
        }
        WebsocketServerUpdateMessage::LiveTaskDel { live_task_id } => {
            live.retain(|x| x.id != live_task_id);
            ServerStateCheckpoint { live, finished }
        }
        WebsocketServerUpdateMessage::LiveTaskDelIns {
            live_task_id_del,
            live_task_id_ins,
        } => {
            let ins_pos = live.iter().position(|x| x.id == live_task_id_ins);
            let del_pos = live.iter().position(|x| x.id == live_task_id_del);

            if let (Some(mut ins_pos), Some(del_pos)) = (ins_pos, del_pos) {
                if ins_pos > del_pos {
                    ins_pos -= 1;
                }
                let removed = live.remove(del_pos).unwrap();
                live.insert(ins_pos, removed);
            }
            ServerStateCheckpoint { live, finished }
        }
        WebsocketServerUpdateMessage::FinishedTaskPush {
            finished_task_id,
            value,
            status,
        } => {
            finished.push(FinishedTask {
                id: finished_task_id,
                value,
                status,
            });
            ServerStateCheckpoint { live, finished }
        }
        WebsocketServerUpdateMessage::FinishedTaskPushComplete {
            live_task_id,
            finished_task_id,
            status,
        } => {
            if let Some(pos_in_live) = live.iter().position(|x| x.id == live_task_id) {
                finished.push(FinishedTask {
                    id: finished_task_id,
                    value: live.remove(pos_in_live).unwrap().value,
                    status,
                });
            }
            ServerStateCheckpoint { live, finished }
        }
    }
}

// in case there is more than one websocket connecting to the same server, we need a way to order sql reads and writes to the same user
// to do this, we'll initialize a worker that is responsible for writing to sql and updating the rw lock
// how do we know wrt
async fn user_worker() {}
