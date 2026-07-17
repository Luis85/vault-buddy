//! Model-management IPC: list the transcription model cache and delete a
//! cached artifact so the next job re-downloads it (SHA-verified) — the
//! user-facing remedy for a suspect cached model (docs/Gaps.md GAP-14).

use std::path::PathBuf;
use tauri::AppHandle;
use vault_buddy_transcribe::model::{list_artifacts_in, model_artifacts, model_dir};

use crate::transcription::{is_any_transcription_active, request_model_purge};

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatusDto {
    pub id: String,
    pub file_name: String,
    pub present: bool,
    pub size_bytes: Option<u64>,
    pub approx_download_bytes: u64,
}

/// Strict id → file-name lookup. Deliberately NOT ModelTier::from_str,
/// which defaults unknown input to Small — a typo'd id must be an error,
/// never a deletion of the wrong model.
fn artifact_file_name(id: &str) -> Option<&'static str> {
    model_artifacts()
        .into_iter()
        .find(|a| a.id == id)
        .map(|a| a.file_name)
}

#[tauri::command]
pub fn list_transcription_models() -> Vec<ModelStatusDto> {
    let approx: std::collections::HashMap<&str, u64> = model_artifacts()
        .into_iter()
        .map(|a| (a.id, a.approx_download_bytes))
        .collect();
    let Some(dir) = model_dir() else {
        return Vec::new(); // unresolvable %APPDATA%: an empty card, not an error
    };
    list_artifacts_in(&dir)
        .into_iter()
        .map(|s| ModelStatusDto {
            approx_download_bytes: approx.get(s.id.as_str()).copied().unwrap_or(0),
            id: s.id,
            file_name: s.file_name,
            present: s.present,
            size_bytes: s.size_bytes,
        })
        .collect()
}

/// Async: the bounded retry below sleeps while the worker drops its
/// cached (mmap'd) transcriber — a sync command would block the main
/// thread for up to ~2 s.
#[tauri::command]
pub async fn delete_transcription_model(app: AppHandle, id: String) -> Result<(), String> {
    let file_name = artifact_file_name(&id).ok_or_else(|| format!("Unknown model id: {id}"))?;
    if is_any_transcription_active(&app) {
        return Err("A transcription is running — try again when it finishes.".to_string());
    }
    let dir = model_dir().ok_or("cannot resolve model directory")?;
    let path: PathBuf = dir.join(file_name);
    request_model_purge(&app, &id);
    tauri::async_runtime::spawn_blocking(move || {
        // Ride out the worker's cache drop: Windows refuses to unlink a
        // file the (idle) worker still has mapped, and the purge request
        // is serviced on its thread's next wake. 20 × 100 ms bounds the
        // wait; NotFound is success (the contract is "the path is clear").
        let mut last_err: Option<std::io::Error> = None;
        for _ in 0..20 {
            match std::fs::remove_file(&path) {
                Ok(()) => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) => {
                    last_err = Some(e);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
        Err(format!(
            "Couldn't delete the model — it is still in use ({}). It will be deletable after the next transcription finishes or an app restart.",
            last_err.map(|e| e.to_string()).unwrap_or_default()
        ))
    })
    .await
    .map_err(|e| format!("delete task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_rejects_unknown_ids_strictly() {
        // ModelTier::from_str defaults unknown input to Small — using it
        // here would let a garbage id delete the Small model. The command
        // must validate against the artifact list instead.
        assert!(artifact_file_name("garbage").is_none());
        assert!(artifact_file_name("").is_none());
        assert_eq!(artifact_file_name("small").unwrap(), "ggml-small.bin");
        assert_eq!(artifact_file_name("vad").unwrap(), "ggml-silero-v5.1.2.bin");
    }
}
