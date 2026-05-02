use crate::domain::errors::AppError;
use crate::state::AppState;
use std::fs::OpenOptions;
use std::io::Write;
use std::process::Command;
use whisper_rs::{FullParams, SamplingStrategy};

pub fn transcribe_audio_job(state: AppState, job_id: String) -> Result<(), AppError> {
    let temp_dir = std::env::temp_dir();
    let input_path = temp_dir.join(format!("whisper_input_{}", job_id));
    let output_path = temp_dir.join(format!("whisper_output_{}.raw", job_id));
    let text_output_path = temp_dir.join(format!("{}.txt", job_id));

    tracing::info!("🔧 Job {}: Starting ffmpeg audio conversion...", job_id);

    let ffmpeg_result = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            input_path.to_str().unwrap_or(""),
            "-ar",
            "16000",
            "-ac",
            "1",
            "-f",
            "f32le",
            output_path.to_str().unwrap_or(""),
        ])
        .output()
        .map_err(|e| AppError::InternalError(anyhow::anyhow!("Failed to start ffmpeg: {}", e)))?;

    let _ = std::fs::remove_file(&input_path);

    if !ffmpeg_result.status.success() {
        let _ = std::fs::remove_file(&output_path);
        let stderr = String::from_utf8_lossy(&ffmpeg_result.stderr);
        return Err(AppError::BadRequest(format!(
            "Failed to process audio file: {}",
            stderr
        )));
    }

    let raw_bytes = std::fs::read(&output_path).map_err(|e| {
        AppError::InternalError(anyhow::anyhow!("Failed to read ffmpeg output: {}", e))
    })?;

    let _ = std::fs::remove_file(&output_path);

    let samples: Vec<f32> = raw_bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();

    tracing::info!("📖 Job {}: Loaded {} samples", job_id, samples.len());

    let mut whisper_state = state
        .whisper_ctx
        .create_state()
        .map_err(|e| AppError::WhisperError(format!("Failed to create state: {}", e)))?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("en"));
    params.set_n_threads(1); // Normal way, using 1 processor
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<i32>();
    let conn_clone = state.db_conn.clone();
    let job_id_clone = job_id.clone();

    // Spawn thread to handle DB updates to avoid blocking the C++ callback thread
    let db_task = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            use sea_orm::{EntityTrait, ActiveModelTrait, Set};
            use crate::domain::models::job::{Entity as JobEntity, ActiveModel as JobActiveModel};
            while let Some(prog) = rx.recv().await {
                if let Some(j) = JobEntity::find_by_id(&job_id_clone).one(&conn_clone).await.unwrap_or(None) {
                    let mut am: JobActiveModel = j.into();
                    am.progress = Set(prog as i64);
                    let _ = am.update(&conn_clone).await;
                }
            }
        });
    });

    let mut last_progress = -1;
    params.set_progress_callback_safe(move |progress: i32| {
        if progress > last_progress {
            let _ = tx.send(progress);
            last_progress = progress;
        }
    });

    let text_path_clone = text_output_path.clone();
    params.set_segment_callback_safe_lossy(move |seg: whisper_rs::SegmentCallbackData| {
        let text = seg.text.trim();
        if !text.is_empty() {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&text_path_clone)
            {
                let _ = writeln!(file, "{}", text);
            }
        }
    });

    whisper_state
        .full(params, &samples)
        .map_err(|e| AppError::WhisperError(format!("Inference failed: {}", e)))?;

    // The channel is closed when `params` is dropped here.
    // However, `params` actually drops after `full()` executes, closing `tx`.
    // Wait for the DB task to gracefully complete.
    let _ = db_task.join();

    Ok(())
}
