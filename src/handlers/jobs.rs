use crate::{
    domain::{errors::AppError, models::Job},
    domain::models::job::{Entity as JobEntity, Column as JobColumn},
    state::AppState,
};
use axum::{
    extract::{Path, State},
    Json,
};
use sea_orm::{EntityTrait, QueryOrder};

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
        .map_err(|e| AppError::InternalError(anyhow::anyhow!("Failed to fetch jobs: {}", e)))?;

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
        .ok_or_else(|| AppError::BadRequest("Job not found".to_string()))?;

    Ok(Json(job))
}
