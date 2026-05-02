use crate::{
    domain::models::{TranscriptionResponse, TranscriptionSegment, Job, JobResponse},
    handlers::{
        transcribe::{transcribe_handler, __path_transcribe_handler},
        jobs::{get_jobs_handler, get_job_handler, get_job_transcription_handler, cancel_job_handler, delete_job_handler, __path_get_jobs_handler, __path_get_job_handler, __path_get_job_transcription_handler, __path_cancel_job_handler, __path_delete_job_handler},
    },
    state::AppState,
};
use axum::{extract::DefaultBodyLimit, routing::{get, post}, Router};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[derive(OpenApi)]
#[openapi(
    paths(transcribe_handler, get_jobs_handler, get_job_handler, get_job_transcription_handler, cancel_job_handler, delete_job_handler),
    components(schemas(TranscriptionResponse, TranscriptionSegment, Job, JobResponse))
)]
struct ApiDoc;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/transcribe", post(transcribe_handler))
        .route("/jobs", get(get_jobs_handler))
        .route("/jobs/:id", get(get_job_handler).delete(delete_job_handler))
        .route("/jobs/:id/transcription", get(get_job_transcription_handler))
        .route("/jobs/:id/cancel", post(cancel_job_handler))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(DefaultBodyLimit::max(500 * 1024 * 1024)) // Set limit to 500MB
        .with_state(state)
}
