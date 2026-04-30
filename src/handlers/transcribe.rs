use crate::{
    domain::{errors::AppError, models::TranscriptionResponse},
    services::whisper::transcribe_audio,
    state::AppState,
};
use axum::{
    extract::{Multipart, State},
    Json,
};
use bytes::Bytes;
use std::process::Command;
use std::io::Write;
use utoipa::ToSchema;

/// Transcribe an uploaded audio file (WAV, MP3, M4A, AAC, etc.).
#[utoipa::path(
    post,
    path = "/transcribe",
    request_body(content = inline(UploadAudio), content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Audio successfully transcribed", body = TranscriptionResponse),
        (status = 400, description = "Bad request (e.g., invalid file)"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn transcribe_handler(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<TranscriptionResponse>, AppError> {
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

    // We do audio processing and transcription in a blocking task
    // because ffmpeg conversion and Whisper inference are CPU bound.
    let total_start = std::time::Instant::now();
    let response = tokio::task::spawn_blocking(move || {
        // Write uploaded bytes to a temp file to avoid pipe deadlocks on large files.
        let temp_dir = std::env::temp_dir();
        let request_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let input_path = temp_dir.join(format!("whisper_input_{}", request_id));
        let output_path = temp_dir.join(format!("whisper_output_{}.raw", request_id));

        // Write input file
        tracing::info!("💾 Writing uploaded file to temp storage...");
        let mut input_file = std::fs::File::create(&input_path).map_err(|e| {
            AppError::InternalError(anyhow::anyhow!("Failed to create temp input file: {}", e))
        })?;
        input_file.write_all(&file_bytes).map_err(|e| {
            AppError::InternalError(anyhow::anyhow!("Failed to write temp input file: {}", e))
        })?;
        drop(input_file);
        // Drop the uploaded bytes early to free memory before transcription
        drop(file_bytes);
        tracing::info!("💾 Temp file written successfully");

        tracing::info!("🔧 Starting ffmpeg audio conversion (resampling to 16kHz mono)...");
        let ffmpeg_start = std::time::Instant::now();

        // Run ffmpeg: read from temp file, write to temp file
        let ffmpeg_result = Command::new("ffmpeg")
            .args([
                "-y",                                     // overwrite output
                "-i", input_path.to_str().unwrap_or(""),  // read from temp file
                "-ar", "16000",                           // resample to 16kHz
                "-ac", "1",                               // convert to mono
                "-f", "f32le",                            // raw 32-bit float output
                output_path.to_str().unwrap_or(""),       // write to temp file
            ])
            .output()
            .map_err(|e| {
                AppError::InternalError(anyhow::anyhow!(
                    "Failed to start ffmpeg. Ensure ffmpeg is installed and in PATH: {}", e
                ))
            })?;

        // Clean up input file immediately
        let _ = std::fs::remove_file(&input_path);

        if !ffmpeg_result.status.success() {
            let _ = std::fs::remove_file(&output_path);
            let stderr = String::from_utf8_lossy(&ffmpeg_result.stderr);
            return Err(AppError::BadRequest(format!(
                "Failed to process audio file (is it a valid audio format?): {}",
                stderr
            )));
        }

        let ffmpeg_elapsed = ffmpeg_start.elapsed();
        tracing::info!("🔧 ffmpeg conversion complete in {:.1}s", ffmpeg_elapsed.as_secs_f64());

        // Read the raw f32le output file
        tracing::info!("📖 Reading converted audio samples...");
        let raw_bytes = std::fs::read(&output_path).map_err(|e| {
            AppError::InternalError(anyhow::anyhow!("Failed to read ffmpeg output file: {}", e))
        })?;

        let output_size_mb = raw_bytes.len() as f64 / (1024.0 * 1024.0);

        // Clean up output file
        let _ = std::fs::remove_file(&output_path);

        // Convert the raw f32le bytes into Vec<f32>
        let samples: Vec<f32> = raw_bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();

        let duration_secs = samples.len() as f64 / 16000.0;
        tracing::info!(
            "📖 Loaded {} samples ({:.1} MB raw) = {:.0}m {:.0}s of audio",
            samples.len(),
            output_size_mb,
            (duration_secs / 60.0).floor(),
            duration_secs % 60.0
        );

        // Run transcription
        transcribe_audio(state.whisper_ctx.clone(), samples)
    })
    .await
    .map_err(|e| AppError::InternalError(e.into()))??;

    let total_elapsed = total_start.elapsed();
    tracing::info!(
        "🏁 Total request completed in {:.0}m {:.0}s | Segments: {} | Text length: {} chars",
        (total_elapsed.as_secs_f64() / 60.0).floor(),
        total_elapsed.as_secs_f64() % 60.0,
        response.segments.len(),
        response.full_text.len()
    );

    Ok(Json(response))
}

// Dummy struct to help utoipa generate multipart form schema
#[derive(ToSchema)]
#[allow(dead_code)]
struct UploadAudio {
    /// Audio file to transcribe (WAV, MP3, M4A, AAC, etc.)
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}
