use crate::domain::{
    errors::AppError,
    models::{TranscriptionResponse, TranscriptionSegment},
};
use std::sync::Arc;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext};

/// The duration of each audio chunk in seconds for parallel processing.
/// Whisper processes audio in 30-second windows internally, so we use
/// a multiple of 30 for clean boundaries. 5 minutes per chunk is a good
/// balance between parallelism and overhead.
const CHUNK_DURATION_SECS: usize = 300; // 5 minutes

/// Overlap between chunks in seconds to avoid cutting words at boundaries.
const CHUNK_OVERLAP_SECS: usize = 5;

/// Transcribe a single chunk of audio using the given Whisper context.
/// Returns segments with timestamps offset by `time_offset_cs` (centiseconds).
fn transcribe_chunk(
    ctx: &Arc<WhisperContext>,
    chunk: &[f32],
    chunk_index: usize,
    total_chunks: usize,
    time_offset_cs: i64,
    n_threads: i32,
) -> Result<Vec<TranscriptionSegment>, AppError> {
    let chunk_duration = chunk.len() as f64 / 16000.0;
    tracing::info!(
        "🔄 Chunk {}/{}: Starting transcription ({:.0}s of audio, offset {:.0}s, {} threads)",
        chunk_index + 1,
        total_chunks,
        chunk_duration,
        time_offset_cs as f64 / 100.0,
        n_threads
    );

    let mut state = ctx
        .create_state()
        .map_err(|e| AppError::WhisperError(format!("Failed to create Whisper state: {}", e)))?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("en"));
    params.set_n_threads(n_threads);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    // Progress callback for this chunk
    let ci = chunk_index;
    let tc = total_chunks;
    let chunk_start = std::time::Instant::now();
    params.set_progress_callback_safe(move |progress: i32| {
        let elapsed = chunk_start.elapsed().as_secs_f64();
        if progress > 0 && progress % 20 == 0 {
            let eta = elapsed / (progress as f64 / 100.0) - elapsed;
            tracing::info!(
                "📊 Chunk {}/{}: {}% | Elapsed: {:.0}s | ETA: {:.0}s",
                ci + 1, tc, progress, elapsed, eta
            );
        }
    });

    // Segment callback: logs each segment in real-time
    let offset = time_offset_cs;
    params.set_segment_callback_safe_lossy(move |seg: whisper_rs::SegmentCallbackData| {
        let start_ms = (seg.start_timestamp + offset) * 10;
        let end_ms = (seg.end_timestamp + offset) * 10;
        tracing::info!(
            "📝 [{}:{:02}.{:03} → {}:{:02}.{:03}] {}",
            start_ms / 60000,
            (start_ms % 60000) / 1000,
            start_ms % 1000,
            end_ms / 60000,
            (end_ms % 60000) / 1000,
            end_ms % 1000,
            seg.text.trim()
        );
    });

    let chunk_inference_start = std::time::Instant::now();
    state
        .full(params, chunk)
        .map_err(|e| AppError::WhisperError(format!("Chunk {} failed: {}", chunk_index + 1, e)))?;

    let chunk_elapsed = chunk_inference_start.elapsed();
    tracing::info!(
        "✅ Chunk {}/{}: Done in {:.1}s ({:.1}x realtime)",
        chunk_index + 1,
        total_chunks,
        chunk_elapsed.as_secs_f64(),
        chunk_duration / chunk_elapsed.as_secs_f64()
    );

    // Extract segments with adjusted timestamps
    let num_segments = state.full_n_segments();
    let mut segments = Vec::new();

    for i in 0..num_segments {
        let segment = state
            .get_segment(i)
            .ok_or_else(|| AppError::WhisperError(format!("Failed to get segment {}", i)))?;

        let text = segment
            .to_str_lossy()
            .map_err(|e| AppError::WhisperError(format!("Failed to get segment text: {}", e)))?
            .to_string();

        // Offset timestamps to the correct position in the full audio
        let start_timestamp = segment.start_timestamp() + time_offset_cs;
        let end_timestamp = segment.end_timestamp() + time_offset_cs;

        segments.push(TranscriptionSegment {
            start_timestamp,
            end_timestamp,
            text,
        });
    }

    Ok(segments)
}

pub fn transcribe_audio(
    ctx: Arc<WhisperContext>,
    audio_data: Vec<f32>,
) -> Result<TranscriptionResponse, AppError> {
    let total_samples = audio_data.len();
    let duration_secs = total_samples as f64 / 16000.0;
    let num_cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    tracing::info!("┌─────────────────────────────────────────────────────┐");
    tracing::info!("│  🎙️  WHISPER TRANSCRIPTION STARTED                  │");
    tracing::info!(
        "│  Audio: {:.1}s ({:.1} min) | CPU threads: {}",
        duration_secs, duration_secs / 60.0, num_cpus
    );
    tracing::info!("└─────────────────────────────────────────────────────┘");

    let chunk_samples = CHUNK_DURATION_SECS * 16000;
    let overlap_samples = CHUNK_OVERLAP_SECS * 16000;

    // If the audio is short enough, process it in a single pass (no chunking overhead)
    if total_samples <= chunk_samples + overlap_samples {
        tracing::info!("⚡ Audio is short enough for single-pass processing");
        let segments = transcribe_chunk(
            &ctx,
            &audio_data,
            0, 1,
            0,
            num_cpus as i32,
        )?;

        let full_text = segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");

        tracing::info!("┌─────────────────────────────────────────────────────┐");
        tracing::info!("│  ✅  TRANSCRIPTION COMPLETE                         │");
        tracing::info!(
            "│  Segments: {} | Chars: {}",
            segments.len(), full_text.len()
        );
        tracing::info!("└─────────────────────────────────────────────────────┘");

        return Ok(TranscriptionResponse {
            full_text: full_text.trim().to_string(),
            segments,
        });
    }

    // Split audio into chunks for parallel processing
    let mut chunks: Vec<(usize, usize, i64)> = Vec::new(); // (start_sample, end_sample, time_offset_cs)
    let mut offset = 0;

    while offset < total_samples {
        let end = (offset + chunk_samples).min(total_samples);
        // Time offset in centiseconds (Whisper timestamps are in centiseconds)
        let time_offset_cs = (offset as f64 / 16000.0 * 100.0) as i64;
        chunks.push((offset, end, time_offset_cs));
        if end >= total_samples {
            break;
        }
        // Move forward by chunk size minus overlap
        offset = end - overlap_samples;
    }

    let total_chunks = chunks.len();
    // Divide threads among chunks, but each chunk needs at least 1 thread
    // For parallel processing, we run 2 chunks at a time with half the threads each
    let max_parallel = 2.min(total_chunks);
    let threads_per_chunk = (num_cpus / max_parallel).max(1) as i32;

    tracing::info!(
        "✂️  Splitting into {} chunks ({} min each, {}s overlap) | {} parallel, {} threads/chunk",
        total_chunks,
        CHUNK_DURATION_SECS / 60,
        CHUNK_OVERLAP_SECS,
        max_parallel,
        threads_per_chunk
    );

    // Process chunks in parallel batches
    let mut all_segments: Vec<Vec<TranscriptionSegment>> = (0..total_chunks).map(|_| Vec::new()).collect();

    for batch_start in (0..total_chunks).step_by(max_parallel) {
        let batch_end = (batch_start + max_parallel).min(total_chunks);
        let _batch_size = batch_end - batch_start;

        tracing::info!(
            "🔄 Processing batch: chunks {} to {} (of {})",
            batch_start + 1, batch_end, total_chunks
        );

        // Spawn threads for this batch
        let mut handles = Vec::new();
        for chunk_idx in batch_start..batch_end {
            let (start, end, time_offset) = chunks[chunk_idx];
            let chunk_data = audio_data[start..end].to_vec();
            let ctx_clone = ctx.clone();
            let tc = total_chunks;
            let tpc = threads_per_chunk;

            let handle = std::thread::spawn(move || {
                transcribe_chunk(&ctx_clone, &chunk_data, chunk_idx, tc, time_offset, tpc)
            });
            handles.push((chunk_idx, handle));
        }

        // Collect results
        for (chunk_idx, handle) in handles {
            let segments = handle
                .join()
                .map_err(|_| AppError::WhisperError(format!("Chunk {} thread panicked", chunk_idx + 1)))??;
            all_segments[chunk_idx] = segments;
        }

        tracing::info!(
            "✅ Batch complete ({}/{})",
            batch_end, total_chunks
        );
    }

    // Merge segments from all chunks, removing overlapping duplicates
    tracing::info!("🔗 Merging {} chunks and removing overlaps...", total_chunks);
    let mut merged_segments: Vec<TranscriptionSegment> = Vec::new();

    for (i, chunk_segments) in all_segments.into_iter().enumerate() {
        if i == 0 {
            // First chunk: take all segments
            merged_segments.extend(chunk_segments);
        } else {
            // For subsequent chunks, skip segments that overlap with the previous chunk's end
            let prev_end = merged_segments.last().map(|s| s.end_timestamp).unwrap_or(0);
            for seg in chunk_segments {
                // Only add segments that start after the previous chunk's last segment ended
                if seg.start_timestamp >= prev_end {
                    merged_segments.push(seg);
                }
            }
        }
    }

    let full_text = merged_segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    tracing::info!("┌─────────────────────────────────────────────────────┐");
    tracing::info!("│  ✅  TRANSCRIPTION COMPLETE                         │");
    tracing::info!(
        "│  Segments: {} | Chars: {}",
        merged_segments.len(), full_text.len()
    );
    tracing::info!("└─────────────────────────────────────────────────────┘");

    Ok(TranscriptionResponse {
        full_text: full_text.trim().to_string(),
        segments: merged_segments,
    })
}
