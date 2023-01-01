use std::net::Ipv4Addr;
use std::str::FromStr;

use actix_web::{middleware, web, App, HttpServer};
use clap::Parser;

use auth_service_api::client::AuthService;
use todoproxy_api::response::WebsocketServerUpdateMessage;
use tokio::sync::broadcast;

mod db_types;
mod handlers;
mod utils;

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
    site_external_url: String,
}

#[derive(Clone)]
pub struct AppData {
    pub task_update_tx: broadcast::Sender<WebsocketServerUpdateMessage>,
    pub auth_service: AuthService,
    pub site_external_url: String,
    pub pool: deadpool_postgres::Pool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    env_logger::init();

    let Opts {
        auth_service_url,

        site_external_url,
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
    let auth_service = AuthService::new(&auth_service_url).await;
    log::info!(target:"todoproxy::deadpool", "connected to auth service");

    // broadcast all updates
    // max buffer size of 1000 for now, because I think
    let (task_update_tx, _) = broadcast::channel(1000);

    // start server
    let data = AppData {
        task_update_tx,
        auth_service,
        site_external_url,
        pool,
    };

    HttpServer::new(move || {
        App::new()
            // enable logger
            .wrap(middleware::Logger::default())
            // add data
            .app_data(actix_web::web::Data::new(data.clone()))
            // handle info query
            .service(web::resource("/info").route(web::get().to(handlers::info)))
            // handle ws connection
            .service(web::resource("/ws").route(web::get().to(handlers::ws)))
    })
    .bind((Ipv4Addr::LOCALHOST, port))?
    .run()
    .await?;

    Ok(())
}
