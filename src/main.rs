mod api;
mod domain;
mod handlers;
mod services;
mod state;
mod worker;

use sea_orm::ConnectionTrait;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use whisper_rs::{WhisperContext, WhisperContextParameters};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "audio_transcriber=debug,tower_http=debug,axum=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Ensure models and temp directories exist
    std::fs::create_dir_all("models")?;
    
    // DB setup
    let db_conn = sea_orm::Database::connect("sqlite://jobs.db?mode=rwc").await?;

    db_conn.execute(sea_orm::Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        "CREATE TABLE IF NOT EXISTS jobs (
            id TEXT PRIMARY KEY,
            status TEXT NOT NULL,
            progress INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )",
    ))
    .await?;

    let model_path = "models/ggml-large-v3-turbo.bin";

    tracing::info!("Initializing Whisper context from {}", model_path);
    tracing::info!(
        "Please ensure you have downloaded a ggml model to {}",
        model_path
    );

    // In a real application, you might want to gracefully handle the missing model
    // but for start up, failing fast is usually good.
    let ctx = match WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to load Whisper model at {}: {}", model_path, e);
            tracing::warn!("You can download a model using: wget https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin -o models/ggml-large-v3-turbo.bin");
            return Err(e.into());
        }
    };

    let state = state::AppState {
        whisper_ctx: Arc::new(ctx),
        db_conn,
    };

    // Start worker thread
    tokio::spawn(worker::start_worker(state.clone()));

    let app = api::create_router(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    tracing::info!("Server listening on {}", listener.local_addr()?);
    tracing::info!("Swagger UI available at http://localhost:3000/swagger-ui/");

    axum::serve(listener, app).await?;

    Ok(())
}
