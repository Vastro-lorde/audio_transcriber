use crate::{
    domain::{errors::AppError, models::{Job, TranscriptionResponse}},
    domain::models::job::{Entity as JobEntity, Column as JobColumn, ActiveModel as JobActiveModel},
    state::AppState,
};
use axum::{
    extract::{Path, State},
    Json,
};
use sea_orm::{EntityTrait, QueryOrder, ActiveModelTrait, Set};

#[utoipa::path(
    get,
    path = "/jobs",
    responses(
        (status = 200, description = "List of all jobs", body = Vec<Job>),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_jobs_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<Job>>, AppError> {
    let jobs = JobEntity::find()
        .order_by_desc(JobColumn::CreatedAt)
        .all(&state.db_conn)
        .await
        .map_err(|e| AppError::InternalError(anyhow::anyhow!("Failed to fetch jobs: {}", e)))?
        .into_iter()
        .map(Job::from)
        .collect();

    Ok(Json(jobs))
}

#[utoipa::path(
    get,
    path = "/jobs/{id}",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    responses(
        (status = 200, description = "Job details", body = Job),
        (status = 404, description = "Job not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_job_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Job>, AppError> {
    let job = JobEntity::find_by_id(id)
        .one(&state.db_conn)
        .await
        .map_err(|e| AppError::InternalError(anyhow::anyhow!("Failed to fetch job: {}", e)))?
        .map(Job::from)
        .ok_or_else(|| AppError::BadRequest("Job not found".to_string()))?;

    Ok(Json(job))
}

#[utoipa::path(
    get,
    path = "/jobs/{id}/transcription",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    responses(
        (status = 200, description = "Transcription result", body = TranscriptionResponse),
        (status = 404, description = "Job not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_job_transcription_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TranscriptionResponse>, AppError> {
    let _job = JobEntity::find_by_id(&id)
        .one(&state.db_conn)
        .await
        .map_err(|e| AppError::InternalError(anyhow::anyhow!("Failed to fetch job: {}", e)))?
        .ok_or_else(|| AppError::BadRequest("Job not found".to_string()))?;

    let temp_dir = std::env::temp_dir();
    let text_output_path = temp_dir.join(format!("{}.txt", id));

    let full_text = tokio::fs::read_to_string(&text_output_path)
        .await
        .unwrap_or_default();

    Ok(Json(TranscriptionResponse {
        full_text,
        segments: vec![],
    }))
}

#[utoipa::path(
    post,
    path = "/jobs/{id}/cancel",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    responses(
        (status = 200, description = "Job cancelled successfully"),
        (status = 404, description = "Job not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn cancel_job_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Job>, AppError> {
    let job = JobEntity::find_by_id(&id)
        .one(&state.db_conn)
        .await
        .map_err(|e| AppError::InternalError(anyhow::anyhow!("Failed to fetch job: {}", e)))?
        .ok_or_else(|| AppError::BadRequest("Job not found".to_string()))?;

    if job.status == "completed" || job.status == "failed" {
        return Err(AppError::BadRequest("Job is already finished".to_string()));
    }

    let mut active_job: JobActiveModel = job.into();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    active_job.status = Set("cancelled".to_string());
    active_job.updated_at = Set(now);
    
    let updated_job = active_job.update(&state.db_conn).await
        .map_err(|e| AppError::InternalError(anyhow::anyhow!("Failed to update job: {}", e)))?;

    Ok(Json(Job::from(updated_job)))
}

#[utoipa::path(
    delete,
    path = "/jobs/{id}",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    responses(
        (status = 200, description = "Job deleted successfully"),
        (status = 404, description = "Job not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn delete_job_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<()>, AppError> {
    let _job = JobEntity::find_by_id(&id)
        .one(&state.db_conn)
        .await
        .map_err(|e| AppError::InternalError(anyhow::anyhow!("Failed to fetch job: {}", e)))?
        .ok_or_else(|| AppError::BadRequest("Job not found".to_string()))?;

    // Try to delete temporary files associated with the job
    let temp_dir = std::env::temp_dir();
    let _ = tokio::fs::remove_file(temp_dir.join(format!("whisper_input_{}", id))).await;
    let _ = tokio::fs::remove_file(temp_dir.join(format!("whisper_output_{}.raw", id))).await;
    let _ = tokio::fs::remove_file(temp_dir.join(format!("{}.txt", id))).await;

    let res = JobEntity::delete_by_id(&id).exec(&state.db_conn).await
        .map_err(|e| AppError::InternalError(anyhow::anyhow!("Failed to delete job: {}", e)))?;

    if res.rows_affected == 0 {
        return Err(AppError::BadRequest("Job not found".to_string()));
    }

    Ok(Json(()))
}
