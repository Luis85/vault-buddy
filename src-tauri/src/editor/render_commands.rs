//! The render and product commands (Task 46; F-41, F-42; ADR §3.3):
//! `editor_start_render`, `editor_get_products`, `editor_restore_product`,
//! plus the production toolchain behind `render_jobs::RenderRunner`.
//!
//! Every `#[tauri::command]` here takes `window: WebviewWindow` and calls
//! `authz::require_editor_window(&window)?` FIRST (R8, `authz_guard.rs`).
//! The rules live in `render_jobs`; this file resolves ffmpeg, spawns the
//! named `editor-render` thread and hands the Channel over. Jobs never use
//! events (ADR invariant 7): progress travels on the caller's Channel only.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, WebviewWindow};
use vault_buddy_core::editor::render_plan::RenderPlan;
use vault_buddy_core::editor::{EditorError, EditorErrorCode, EditorProjection};
use vault_buddy_screen::ffmpeg_args::EncodeSettings;
use vault_buddy_screen::render::run::{render, render_refusal, RenderRequestNative};
use vault_buddy_screen::render::FfmpegCapabilities;
use vault_buddy_screen::ScreenError;

use super::authz::require_editor_window;
use super::media_jobs::{JobKind, JobPhase, JobProgressDto, JobReporter, JobTerminal};
use super::render_jobs::{
    begin_render, no_ffmpeg, products_in, restore_in, run_render_job, ProductDto, RenderJob,
    RenderRequest, RenderRunner, RenderStarted, RenderWork,
};
use super::EditorState;
use crate::ffmpeg::{probe_capabilities, resolve_working_ffmpeg, FfmpegTools};

fn internal(message: impl Into<String>) -> EditorError {
    EditorError::new(EditorErrorCode::Internal, message)
}

fn local_data(app: &AppHandle) -> Result<PathBuf, EditorError> {
    app.path()
        .app_local_data_dir()
        .map_err(|e| internal(format!("Could not resolve the app data directory: {e}")))
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, EditorError> + Send + 'static,
) -> Result<T, EditorError> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| internal(format!("The editor task failed: {e}")))?
}

/// The production runner: the resolved ffmpeg, its probed capabilities,
/// and `screen::render::run::render` (which refuses, writes the ASS
/// documents, runs the child under `RENDER_RUNNER`'s thread names and
/// verifies the output with ffprobe).
pub(crate) struct FfmpegRenderer {
    tools: FfmpegTools,
    caps: FfmpegCapabilities,
}

impl FfmpegRenderer {
    /// Resolve ffmpeg (`encoderUnavailable` naming the fix when there is
    /// none) and probe what it can do. Spawns children: blocking pool only.
    pub(crate) fn resolve() -> Result<Self, EditorError> {
        let tools = resolve_working_ffmpeg().ok_or_else(no_ffmpeg)?;
        let caps = probe_capabilities(&tools);
        Ok(Self { tools, caps })
    }
}

impl RenderRunner for FfmpegRenderer {
    fn h264_encoder(&self) -> String {
        self.tools.h264_encoder.clone().unwrap_or_default()
    }

    fn refusal(&self, plan: &RenderPlan, settings: &EncodeSettings) -> Option<String> {
        render_refusal(plan, &self.caps, &settings.h264_encoder)
    }

    fn free_bytes(&self, dir: &Path) -> Option<u64> {
        vault_buddy_screen::disk::free_bytes(dir)
    }

    fn render(
        &self,
        work: &RenderWork<'_>,
        cancel: &AtomicBool,
        on_progress: &mut dyn FnMut(u64),
    ) -> Result<u64, ScreenError> {
        let request = RenderRequestNative {
            ffmpeg: Path::new(&self.tools.ffmpeg),
            ffprobe: Path::new(&self.tools.ffprobe),
            plan: work.plan,
            inputs: work.inputs,
            job_dir: work.job_dir,
            dest: work.dest,
            fontsdir: None,
            caps: &self.caps,
            settings: work.settings.clone(),
        };
        render(request, cancel, on_progress).map(|outcome| outcome.duration_ms)
    }
}

/// Run `job` on the named `editor-render` thread. A thread that cannot be
/// spawned still ENDS the job it registered (`failed`), so the registry
/// never shows a render running forever.
fn spawn_render(
    app: AppHandle,
    job: RenderJob,
    runner: FfmpegRenderer,
    channel: Channel<JobProgressDto>,
) -> Result<(), EditorError> {
    let (session_id, job_id) = (job.session_id.clone(), job.job_id.clone());
    let fallback = channel.clone();
    let worker = {
        let app = app.clone();
        move || run_render_job(&app.state::<EditorState>(), job, &runner, &channel)
    };
    if let Err(e) = std::thread::Builder::new()
        .name("editor-render".into())
        .spawn(worker)
    {
        log::error!("editor render: could not start the render thread: {e}");
        let error = internal(format!("The render could not start: {e}"));
        let state = app.state::<EditorState>();
        JobReporter::new(
            &state.jobs,
            &fallback,
            &session_id,
            &job_id,
            JobKind::Render,
        )
        .finish(
            JobPhase::Failed,
            JobTerminal {
                error: Some(error.clone()),
                ..JobTerminal::default()
            },
        );
        return Err(error);
    }
    Ok(())
}

/// ASYNC (ADR §3.3): every refusal runs on the blocking pool (resolving
/// ffmpeg spawns probes); the render itself runs on `editor-render` and
/// answers `{ jobId, revision }` at once.
#[tauri::command]
pub async fn editor_start_render(
    window: WebviewWindow,
    app: AppHandle,
    request: RenderRequest,
    on_progress: Channel<JobProgressDto>,
) -> Result<RenderStarted, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || {
        let (job, runner) = begin_render(
            &app.state::<EditorState>(),
            &root,
            &request,
            FfmpegRenderer::resolve,
        )?;
        let started = RenderStarted {
            job_id: job.job_id.clone(),
            revision: job.revision,
        };
        spawn_render(app.clone(), job, runner, on_progress)?;
        Ok(started)
    })
    .await
}

/// ASYNC: reads `products.json` and stats each product's file.
#[tauri::command]
pub async fn editor_get_products(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
) -> Result<Vec<ProductDto>, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || products_in(&app.state::<EditorState>(), &root, &session_id)).await
}

/// ASYNC: reads `products.json` (a snapshot can be megabytes) and applies
/// the restore as ONE edit.
#[tauri::command]
pub async fn editor_restore_product(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    expected_revision: u64,
    product_id: String,
    command_id: String,
) -> Result<EditorProjection, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || {
        restore_in(
            &app.state::<EditorState>(),
            &root,
            &session_id,
            expected_revision,
            &product_id,
            &command_id,
        )
    })
    .await
}
