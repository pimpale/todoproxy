#![feature(try_blocks)]
mod db_types;
mod handlers;
mod task_updates;
mod utils;
mod api;

mod checkpoint_service;
mod operation_service;



use std::collections::HashMap;
use std::str::FromStr;
use std::{net::Ipv4Addr, sync::Arc};

use actix_cors::Cors;
use actix_web::{App, HttpServer, middleware, web};
use auth_service_api::response::User;
use clap::Parser;

use auth_service_api::client::AuthService;
use api::{StateSnapshot, WebsocketOp};
use tokio::sync::Mutex;
use tokio::sync::broadcast;

static SERVICE: &'static str = "todoproxy";
static VERSION_MAJOR: i64 = 0;
static VERSION_MINOR: i64 = 0;
static VERSION_REV: i64 = 1;

#[derive(Parser, Debug, Clone)]
#[clap(about, version, author)]
struct Opts {
    #[clap(long)]
    port: u16,
    #[clap(long)]
    database_url: String,
    #[clap(long)]
    auth_service_url: String,
    #[clap(long)]
    app_pub_origin: String,
}

pub struct PerUserWorkerData {
    // user
    pub user: User,
    // websockets send to this channel when they receive an event
    pub updates_tx: broadcast::Sender<WebsocketOp>,
    // snapshot at the current state of the channel
    pub snapshot: StateSnapshot,
    // id of checkpoint
    pub checkpoint_id: i64,
}

#[derive(Clone)]
pub struct AppData {
    pub user_worker_data: Arc<Mutex<HashMap<i64, Arc<Mutex<PerUserWorkerData>>>>>,
    pub auth_service: AuthService,
    pub app_pub_origin: String,
    pub pool: deadpool_postgres::Pool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    env_logger::init();

    let Opts {
        auth_service_url,
        app_pub_origin,
        port,
        database_url,
    } = Opts::parse();

    // connect to postgres
    let postgres_config = tokio_postgres::Config::from_str(&database_url).map_err(|e| {
        log::error!(target:"todoproxy::deadpool", "couldn't parse database_url: {}", e);
        e
    })?;
    log::info!("parsed database url");

    let mgr = deadpool_postgres::Manager::from_config(
        postgres_config,
        tokio_postgres::NoTls,
        deadpool_postgres::ManagerConfig {
            recycling_method: deadpool_postgres::RecyclingMethod::Fast,
        },
    );

    let pool = deadpool_postgres::Pool::builder(mgr)
        .max_size(16)
        .build()
        .map_err(|e| { log::error!(target:"todoproxy::deadpool", "couldn't build database connection pool: {}", e); e })?;

    log::info!(target:"todoproxy::deadpool", "built database connection pool");

    // open connection to auth service
    let auth_service = AuthService::new(&auth_service_url);
    log::info!(target:"todoproxy::deadpool", "connected to auth service");

    let user_worker_data = Arc::new(Mutex::new(HashMap::new()));

    // start server
    let data = AppData {
        user_worker_data,
        auth_service,
        app_pub_origin,
        pool,
    };

    HttpServer::new(move || {
        // Configure CORS middleware
        let cors = Cors::permissive();
        
        App::new()
            // Add cors middleware before other middleware
            .wrap(cors)
            // enable logger
            .wrap(middleware::Logger::default())
            // add data
            .app_data(actix_web::web::Data::new(data.clone()))
            // handle info query
            .service(web::resource("/public/info").route(web::route().to(handlers::info)))
            // handle ws connection
            .service(
                web::resource("/public/ws/task_updates")
                    .route(web::get().to(handlers::ws_task_updates)),
            )
    })
    .bind((Ipv4Addr::LOCALHOST, port))?
    .run()
    .await?;

    Ok(())
}
