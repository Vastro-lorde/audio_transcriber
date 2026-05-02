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
    use utoipa::ToSchema;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize, ToSchema)]
    #[sea_orm(table_name = "jobs")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub status: String,
        pub progress: i64,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub use job::Model as Job;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct JobResponse {
    pub id: String,
    pub status: String,
}
