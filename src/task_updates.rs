use actix_web::web;
use auth_service_api::response::User;
use futures_util::{stream, stream_select, StreamExt};

use actix_ws::{CloseCode, CloseReason, Message, ProtocolError};
use std::{
    collections::{hash_map::Entry, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use todoproxy_api::{
    request::WebsocketInitMessage, FinishedTask, LiveTask, StateSnapshot, TaskStatus, WebsocketOp,
    WebsocketOpKind,
};
use tokio::sync::{broadcast::Receiver, Mutex};
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream, IntervalStream};

use crate::utils;
use crate::PerUserWorkerData;
use crate::{habitica_integration, habitica_integration_service};
use crate::{
    habitica_integration::client::{Direction, HabiticaClient, HabiticaError},
    handlers::{self, get_user_if_api_key_valid},
};
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

    enum TaskUpdateKind {
        // we need to send a heartbeat
        NeedToSendHeartbeat,
        // we received a message from the client
        ClientMessage(Result<Message, ProtocolError>),
        // we have to handle a broadcast from the server
        ServerUpdate(Result<WebsocketOp, BroadcastStreamRecvError>),
    }

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

                // get recent habitica integration
                let habitica_integration =
                    habitica_integration_service::get_recent_by_user_id(&mut *con, user.user_id)
                        .await
                        .map_err(handlers::report_postgres_err)?
                        .ok_or(AppError::IntegrationNotFound)?;

                // create client
                let habitica_client = habitica_integration::client::HabiticaClient::new(
                    data.author_id.clone(),
                    crate::SERVICE.into(),
                    habitica_integration.user_id,
                    habitica_integration.api_key,
                );

                let tasks = habitica_client
                    .get_user_tasks()
                    .await
                    .map_err(handlers::report_habitica_err)?;

                // create channel
                let (updates_tx, updates_rx) = tokio::sync::broadcast::channel(1000);

                // create snapshot from checkpoint
                let mut snapshot = StateSnapshot {
                    live: VecDeque::new(),
                    finished: VecDeque::new(),
                };

                for task in tasks {
                    if task.completed != Some(true) {
                        snapshot.live.push_back(LiveTask {
                            id: task._id,
                            value: task.text.unwrap_or_default(),
                        });
                    } else {
                        snapshot.finished.push_back(FinishedTask {
                            id: task._id,
                            value: task.text.unwrap_or_default(),
                            status: TaskStatus::Succeeded,
                        });
                    }
                }

                let per_user_worker_data_ref = v.insert(Arc::new(Mutex::new(PerUserWorkerData {
                    updates_tx,
                    snapshot: snapshot.clone(),
                    user,
                    habitica_client,
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
        // add to habitica
        let _ = post_operation(&lock.habitica_client, &lock.snapshot, op.kind.clone()).await;
        // apply operation
        apply_operation(&mut lock.snapshot, op.kind.clone());
        // broadcast
        let _ = lock.updates_tx.send(op);
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

async fn post_operation(
    client: &HabiticaClient,
    StateSnapshot {
        ref finished,
        ref live,
    }: &StateSnapshot,
    op: WebsocketOpKind,
) -> Result<(), HabiticaError> {
    match op {
        WebsocketOpKind::OverwriteState(s) => todo!(),
        WebsocketOpKind::InsLiveTask { value, id } => {
            client.insert_todo(id, value).await?;
        }
        WebsocketOpKind::RestoreFinishedTask { id } => {
            client.score_task(id, Direction::Down).await?;
        }
        WebsocketOpKind::EditLiveTask { id, value } => {
            client.update_task(id, value).await?;
        }
        WebsocketOpKind::DelLiveTask { id } => {
            client.delete_task(id).await?;
        }
        WebsocketOpKind::MvLiveTask { id_ins, id_del } => {
            if let Some(ins_pos) = live.iter().position(|x| x.id == id_ins) {
                client.move_task(id_del, ins_pos as i64).await?;
            }
        }
        WebsocketOpKind::FinishLiveTask { id, status } => {
            client.score_task(id, Direction::Up).await?;
        }
    }
    Ok(())
}
