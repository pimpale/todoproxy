use actix_web::web;
use auth_service_api::response::User;
use futures_util::{stream, stream_select, StreamExt};

use actix_ws::{CloseCode, CloseReason, Message, ProtocolError};
use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use todoproxy_api::{
    request::WebsocketInitMessage, FinishedTask, LiveTask, StateSnapshot, WebsocketOp,
    WebsocketOpKind,
};
use tokio::sync::{broadcast::Receiver, Mutex};
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream, IntervalStream};

use crate::handlers::{self, get_user_if_api_key_valid};
use crate::{checkpoint_service, operation_service, PerUserWorkerData};
use crate::{db_types, utils};
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
    init_msg: WebsocketInitMessage,
    mut session: actix_ws::Session,
    msg_stream: actix_ws::MessageStream,
) {
    log::info!("connected");

    // try block for app
    let maybe_per_user_worker_data: Result<
        (
            Arc<Mutex<PerUserWorkerData>>,
            Receiver<WebsocketOp>,
            StateSnapshot,
        ),
        AppError,
    > = try {
        log::info!("trying to get user");
        let user = get_user_if_api_key_valid(&data.auth_service, init_msg.api_key).await?;
        log::info!("validated conenction for user {}", user.user_id);

        let mut write_guard = data.user_worker_data.lock().await;
        match write_guard.entry(user.user_id) {
            Entry::Vacant(v) => {
                // initialize connection
                let con: &mut tokio_postgres::Client =
                    &mut *data.pool.get().await.map_err(handlers::report_pool_err)?;

                // get recent checkpoint
                let preexisting_checkpoint =
                    checkpoint_service::get_recent_by_user_id(&mut *con, user.user_id)
                        .await
                        .map_err(handlers::report_postgres_err)?;

                // if it doesn't exist, create checkpoint
                let recent_checkpoint = match preexisting_checkpoint {
                    Some(x) => x,
                    None => checkpoint_service::add(
                        &mut *con,
                        user.user_id,
                        StateSnapshot {
                            live: VecDeque::new(),
                            finished: VecDeque::new(),
                        },
                    )
                    .await
                    .map_err(handlers::report_postgres_err)?,
                };

                // get all operations since this checkpoint
                let operations_since_last_checkpoint = operation_service::get_operations_since(
                    &mut *con,
                    recent_checkpoint.checkpoint_id,
                )
                .await
                .map_err(handlers::report_postgres_err)?;

                // create channel
                let (updates_tx, updates_rx) = tokio::sync::broadcast::channel(1000);

                // create snapshot from checkpoint
                let mut snapshot = serde_json::from_str(&recent_checkpoint.jsonval)
                    .map_err(handlers::report_internal_serde_error)?;

                for x in operations_since_last_checkpoint {
                    let op = serde_json::from_str::<WebsocketOp>(&x.jsonval)
                        .map_err(handlers::report_internal_serde_error)?;
                    apply_operation(&mut snapshot, op.kind);
                }

                let per_user_worker_data_ref = v.insert(Arc::new(Mutex::new(PerUserWorkerData {
                    updates_tx,
                    snapshot: snapshot.clone(),
                    user,
                    checkpoint_id: recent_checkpoint.checkpoint_id,
                })));

                (per_user_worker_data_ref.clone(), updates_rx, snapshot)
            }
            Entry::Occupied(o) => {
                let per_user_worker_data_ref = o.get().clone();
                let lock = per_user_worker_data_ref.lock().await;
                let receiver = lock.updates_tx.subscribe();
                let snapshot = lock.snapshot.clone();
                drop(lock);
                (per_user_worker_data_ref, receiver, snapshot)
            }
        }
    };

    let (per_user_worker_data, updates_rx, snapshot) = match maybe_per_user_worker_data {
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

    enum TaskUpdateKind {
        // we need to send a heartbeat
        NeedToSendHeartbeat,
        // we received a message from the client
        ClientMessage(Result<Message, ProtocolError>),
        // we have to handle a broadcast from the server
        ServerUpdate(Result<WebsocketOp, BroadcastStreamRecvError>),
    }

    let mut last_heartbeat = Instant::now();

    let heartbeat_stream = IntervalStream::new(tokio::time::interval(HEARTBEAT_INTERVAL))
        .map(|_| TaskUpdateKind::NeedToSendHeartbeat);
    let client_message_stream = msg_stream.map(|x| TaskUpdateKind::ClientMessage(x));

    // first emit the state set, then start producing actual things
    let server_update_stream = stream::once(async {
        Ok(WebsocketOp {
            alleged_time: utils::current_time_millis(),
            kind: WebsocketOpKind::OverwriteState(snapshot),
        })
    })
    .chain(BroadcastStream::new(updates_rx))
    .map(|x| TaskUpdateKind::ServerUpdate(x));

    // pin stream
    tokio::pin!(server_update_stream);

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
                        if let Err(e) =
                            handle_ws_client_op(data.clone(), per_user_worker_data.clone(), &text)
                                .await
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
                    let jsonval = serde_json::to_string(&op).unwrap();
                    let send_result = session.text(jsonval).await;
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
    per_user_worker_data: Arc<Mutex<PerUserWorkerData>>,
    req: &str,
) -> Result<(), AppError> {
    // try to parse request
    let op = serde_json::from_str::<WebsocketOp>(req).map_err(handlers::report_serde_error)?;

    // establish connection to database
    let con: &mut tokio_postgres::Client =
        &mut *data.pool.get().await.map_err(handlers::report_pool_err)?;
    // lock the per-user lock
    {
        let mut lock = per_user_worker_data.lock().await;
        // add to db
        let dbop = operation_service::add(&mut *con, lock.checkpoint_id, op.clone())
            .await
            .map_err(handlers::report_postgres_err)?;
        // apply operation
        apply_operation(&mut lock.snapshot, op.kind.clone());
        // broadcast
        lock.updates_tx.send(op);
    }

    // create thread server request
    return Ok(());
}

fn apply_operation(
    StateSnapshot {
        ref mut finished,
        ref mut live,
    }: &mut StateSnapshot,
    op: WebsocketOpKind,
) {
    match op {
        WebsocketOpKind::OverwriteState(s) => {
            *live = s.live;
            *finished = s.finished;
        }
        WebsocketOpKind::InsLiveTask { value, id } => {
            live.push_front(LiveTask { id, value });
        }
        WebsocketOpKind::RestoreFinishedTask { id } => {
            // if it was found in the finished list, push it to the front
            if let Some(position) = finished.iter().position(|x| x.id == id) {
                let FinishedTask { id, value, .. } = finished.remove(position).unwrap();
                live.push_front(LiveTask { id, value });
            }
        }
        WebsocketOpKind::EditLiveTask { id, value } => {
            for x in live.iter_mut() {
                if x.id == id {
                    x.value = value;
                    break;
                }
            }
        }
        WebsocketOpKind::DelLiveTask { id } => {
            live.retain(|x| x.id != id);
        }
        WebsocketOpKind::MvLiveTask { id_ins, id_del } => {
            let ins_pos = live.iter().position(|x| x.id == id_ins);
            let del_pos = live.iter().position(|x| x.id == id_del);

            if let (Some(ins_pos), Some(del_pos)) = (ins_pos, del_pos) {
                let removed = live.remove(del_pos).unwrap();
                live.insert(ins_pos, removed);
            }
        }
        WebsocketOpKind::FinishLiveTask { id, status } => {
            if let Some(pos_in_live) = live.iter().position(|x| x.id == id) {
                finished.push_front(FinishedTask {
                    id,
                    value: live.remove(pos_in_live).unwrap().value,
                    status,
                });
            }
        }
    }
}
