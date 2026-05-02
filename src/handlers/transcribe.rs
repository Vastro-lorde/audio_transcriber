use crate::{
    domain::{errors::AppError, models::JobResponse},
    state::AppState,
};
use axum::{
    extract::{Multipart, State},
    Json,
};
use bytes::Bytes;
use std::io::Write;
use utoipa::ToSchema;
use uuid::Uuid;

/// Upload an audio file to be transcribed by Whisper.
///
/// Supported formats include WAV, MP3, M4A, AAC, OGG, and FLAC. 
/// The audio file will be queued for background processing. You can check the progress of the job using the returned Job ID.
#[utoipa::path(
    post,
    path = "/transcribe",
    request_body(content = inline(UploadAudio), content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Audio successfully queued", body = JobResponse),
        (status = 400, description = "Bad request (e.g., invalid file)"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn transcribe_handler(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<JobResponse>, AppError> {
    let mut file_data: Option<Bytes> = None;

    tracing::info!("📥 Receiving file upload...");

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        AppError::BadRequest(format!("Failed to read multipart field: {}", e))
    })? {
        if field.name() == Some("file") {
            let data = field.bytes().await.map_err(|e| {
                AppError::BadRequest(format!("Failed to read file data: {}", e))
            })?;
            file_data = Some(data);
            break;
        }
    }

    let file_bytes = file_data.ok_or_else(|| AppError::BadRequest("Missing 'file' field".to_string()))?;
    let file_size_mb = file_bytes.len() as f64 / (1024.0 * 1024.0);
    tracing::info!("📥 File received: {:.1} MB", file_size_mb);

    let job_id = Uuid::new_v4().to_string();
    let temp_dir = std::env::temp_dir();
    let input_path = temp_dir.join(format!("whisper_input_{}", job_id));

    tracing::info!("💾 Writing uploaded file to temp storage...");
    let mut input_file = std::fs::File::create(&input_path).map_err(|e| {
        AppError::InternalError(anyhow::anyhow!("Failed to create temp input file: {}", e))
    })?;
    input_file.write_all(&file_bytes).map_err(|e| {
        AppError::InternalError(anyhow::anyhow!("Failed to write temp input file: {}", e))
    })?;
    drop(input_file);
    drop(file_bytes);

    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    
    use sea_orm::{ActiveModelTrait, Set};
    use crate::domain::models::job::ActiveModel as JobActiveModel;

    let job_model = JobActiveModel {
        id: Set(job_id.clone()),
        status: Set("pending".to_string()),
        progress: Set(0),
        created_at: Set(now),
        updated_at: Set(now),
        error_message: Set(None),
        ..Default::default()
    };

    job_model.insert(&state.db_conn)
        .await
        .map_err(|e| AppError::InternalError(anyhow::anyhow!("Failed to insert job: {}", e)))?;

    Ok(Json(JobResponse {
        id: job_id,
        status: "pending".to_string(),
        error_message: None,
    }))
}

// Dummy struct to help utoipa generate multipart form schema
#[derive(ToSchema)]
#[allow(dead_code)]
struct UploadAudio {
    /// Audio file to transcribe (WAV, MP3, M4A, AAC, etc.)
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}
