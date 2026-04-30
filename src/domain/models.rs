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
