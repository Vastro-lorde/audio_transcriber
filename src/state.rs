use std::sync::Arc;
use whisper_rs::WhisperContext;

#[derive(Clone)]
pub struct AppState {
    pub whisper_ctx: Arc<WhisperContext>,
}
