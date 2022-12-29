use std::net::Ipv4Addr;
use std::str::FromStr;

use clap::Parser;
use serde::{Deserialize, Serialize};

use auth_service_api::client::AuthService;
use tokio::sync::broadcast;

mod db_types;
mod request;
mod response;
mod handlers;
mod utils;

#[derive(Parser, Debug, Clone)]
#[clap(about, version, author)]
struct Opts {
    #[clap(long)]
    port: u16,
    #[clap(long)]
    database_url: String,
    #[clap(long)]
    auth_service_url: String,
}

#[derive(Debug, Clone)]
pub struct AppData {
    pub live_task_update_tx: broadcast::Sender<response::LiveTaskUpdate>,
    pub finished_task_update_tx: broadcast::Sender<response::Task>,
    pub auth_service: AuthService,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    env_logger::init();

    let Opts { auth_service_url, port, database_url } = Opts::parse();

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
        .map_err(|e| {
        log::error!(target:"todoproxy::deadpool", "couldn't build database connection pool: {}", e);
        e
        })?;
    log::info!(target:"todoproxy::deadpool", "built database connection pool");


    // open connection to auth service
    let auth_service = AuthService::new(&auth_service_url).await;
    log::info!(target:"todoproxy::deadpool", "connected to auth service");


    // start server
    let data = AppData { image, auth_service };
    HttpServer::new(move || {
        App::new()
            .app_data(actix_web::web::Data::new(data.clone()))
            .service(handlers::run_code)
    })
    .bind((Ipv4Addr::LOCALHOST, port))?
    .run()
    .await?;

    Ok(())
}
