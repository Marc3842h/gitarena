#![forbid(unsafe_code)]

use std::borrow::{Borrow, Cow};
use std::env;
use std::fs;
use std::io::stdout;
use std::path::Path;
use std::time::Duration;

use actix_web::{App, HttpServer};
use anyhow::{Context, Result};
use chrono::Local;
use config::Config;
use fern::{Dispatch, log_file};
use lazy_static::lazy_static;
use log::{info, LevelFilter};
use sqlx::postgres::PgPoolOptions;
use actix_session::CookieSession;
use time::Duration as TimeDuration;

mod captcha;
mod config;
mod crypto;
mod error;
mod mail;
mod routes;
mod templates;
//mod repository;
mod user;
mod verification;

type GaE = error::GitArenaError;

lazy_static! {
    static ref CONFIG: Cow<'static, Config> = load_config();
}

#[actix_rt::main]
async fn main() -> Result<()> {
    init_logger()?;

    let db_pool = PgPoolOptions::new()
        .max_connections(num_cpus::get() as u32)
        .connect_timeout(Duration::from_secs(10))
        .connect(&CONFIG.database)
        .await?;

    sqlx::query("select 1;").execute(&db_pool).await.context("Unable to connect to database.")?;

    info!("Successfully connected to database.");

    let bind_address: &str = CONFIG.bind.borrow();

    let server = HttpServer::new(move || {
        let secret = (CONFIG.secret.borrow() as &str).as_bytes();
        let session = CookieSession::signed(secret).name("ga_session").secure(false);
        let persistent_session = CookieSession::signed(secret).name("ga_psession").expires_in_time(TimeDuration::weeks(4)).secure(false);

        App::new()
            .data(db_pool.clone())
            .wrap(session)
            .wrap(persistent_session)
            //.configure(routes::repository::init)
            .configure(routes::user::init)
    }).bind(bind_address).context("Unable to bind HTTP server.")?;

    server.run().await.context("Unable to start HTTP server.")?;

    info!("Thank you and goodbye.");

    Ok(())
}

fn load_config() -> Cow<'static, Config> {
    let cfg_str = env::var("GITARENA_CONFIG").unwrap_or("config.toml".to_owned());
    let cfg_path = Path::new(cfg_str.as_str());

    if !cfg_path.is_file() {
        panic!("Config file does not exist: {}", cfg_path.display());
    }

    let config = match Config::load_from(cfg_path) {
        Ok(config) => config,
        Err(err) => panic!("Unable to load config file: {}", err),
    };

    let secret: &str = config.secret.borrow();

    if secret.is_empty() {
        panic!("Found empty secret in config");
    }

    let secret_bytes = secret.as_bytes();

    if secret_bytes.len() < 32 {
        panic!("Secret in config needs to be at least 32 bytes long");
    }

    config
}

fn init_logger() -> Result<()> {
    let logs_dir = Path::new("logs");

    if !logs_dir.is_dir() {
        // Check if `logs` is a file and not a directory
        if logs_dir.exists() {
            fs::remove_file(logs_dir).context("Unable to delete `logs` file.")?;
        }

        fs::create_dir(logs_dir).context("Unable to create `logs` directory.")?;
    }

    let level = if cfg!(debug_assertions) {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };

    Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}] {} {} - {}",
                record.target(),
                record.level(),
                record.module_path().unwrap_or("null"),
                message
            ))
        })
        .level(level)
        .level_for("sqlx", LevelFilter::Warn)
        .level_for("reqwest", LevelFilter::Info)
        .chain(stdout())
        .chain(log_file(format!("logs/{}.log", Local::now().timestamp_millis()))?)
        .apply()
        .context("Failed to initialize logger.")
}
