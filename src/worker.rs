use crate::state::AppState;
use crate::services::whisper::transcribe_audio_job;
use std::time::Duration;
use sea_orm::{EntityTrait, QueryFilter, QueryOrder, ColumnTrait, ActiveModelTrait, Set};
use crate::domain::models::job::{Entity as JobEntity, Column as JobColumn, ActiveModel as JobActiveModel};

pub async fn start_worker(state: AppState) {
    let conn = state.db_conn.clone();
    
    loop {
        // Poll for pending jobs
        let job = JobEntity::find()
            .filter(JobColumn::Status.eq("pending"))
            .order_by_asc(JobColumn::CreatedAt)
            .one(&conn)
            .await
            .unwrap_or(None);

        if let Some(job) = job {
            tracing::info!("Worker picked up job: {}", job.id);
            
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
            
            // Set status to processing
            let mut active_job: JobActiveModel = job.clone().into();
            active_job.status = Set("processing".to_string());
            active_job.updated_at = Set(now);
            let _ = active_job.update(&conn).await;

            let job_id_clone = job.id.clone();
            let state_clone = state.clone();
            
            // Run processing in blocking task
            let res = tokio::task::spawn_blocking(move || {
                transcribe_audio_job(state_clone, job_id_clone)
            }).await;

            let final_status = match res {
                Ok(Ok(_)) => "completed",
                _ => "failed",
            };

            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
            // Update status
            if let Some(j) = JobEntity::find_by_id(&job.id).one(&conn).await.unwrap_or(None) {
                let mut am: JobActiveModel = j.into();
                am.status = Set(final_status.to_string());
                am.updated_at = Set(now);
                let _ = am.update(&conn).await;
            }
                
            tracing::info!("Worker finished job {}: {}", job.id, final_status);
        } else {
            // Sleep a bit before polling again
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
}
