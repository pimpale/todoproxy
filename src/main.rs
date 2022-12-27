use std::net::Ipv4Addr;
use std::str::FromStr;

use actix_web::http::StatusCode;
use actix_web::{error, App, HttpResponse, HttpServer};
use clap::Parser;
use derive_more::Display;
use log::info;
use serde::{Deserialize, Serialize};

use auth_service_api::client::AuthService;
use tokio::sync::broadcast;

mod handlers;
mod utils;

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

impl error::ResponseError for AppError {
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
    pub task_insert_tx : broadcast::Sender<response::Task>,
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
