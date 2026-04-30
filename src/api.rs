use crate::{
    domain::models::{TranscriptionResponse, TranscriptionSegment},
    handlers::transcribe::{transcribe_handler, __path_transcribe_handler},
    state::AppState,
};
use axum::{extract::DefaultBodyLimit, routing::post, Router};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[derive(OpenApi)]
#[openapi(
    paths(transcribe_handler),
    components(schemas(TranscriptionResponse, TranscriptionSegment))
)]
struct ApiDoc;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/transcribe", post(transcribe_handler))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(DefaultBodyLimit::max(500 * 1024 * 1024)) // Set limit to 500MB
        .with_state(state)
}
