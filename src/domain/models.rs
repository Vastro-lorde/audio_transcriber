use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TranscriptionSegment {
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TranscriptionResponse {
    pub full_text: String,
    pub segments: Vec<TranscriptionSegment>,
}

pub mod job {
    use sea_orm::entity::prelude::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "jobs")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub status: String,
        pub progress: i64,
        pub created_at: i64,
        pub updated_at: i64,
        pub error_message: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Job {
    pub id: String,
    pub status: String,
    pub progress: i64,
    pub created_at: i64,
    pub updated_at: i64,
    #[schema(example = "File format not supported")]
    pub error_message: Option<String>,
}

impl From<job::Model> for Job {
    fn from(model: job::Model) -> Self {
        Self {
            id: model.id,
            status: model.status,
            progress: model.progress,
            created_at: model.created_at,
            updated_at: model.updated_at,
            error_message: model.error_message,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct JobResponse {
    pub id: String,
    pub status: String,
    #[schema(example = "File format not supported")]
    pub error_message: Option<String>,
}
