//! Recording session: a worker thread pulls raw chunks from each source,
//! converts them to 44.1 kHz mono, mixes to stereo, streams MP3 bytes into
//! the hidden .part file (flush ~1 s, fsync ~30 s), and finalizes with the
//! stop-time reservation + rename-retry. Sources are plain mpsc channels so
//! the whole session is testable without audio hardware.

use crate::encoder::Mp3Encoder;
use crate::mixer;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use vault_buddy_core::capture_note::{render_note, write_note_collision_safe, NoteMeta};

pub enum SourceMsg {
    Samples(Vec<f32>),
    Lost,
}

/// Session control messages. One channel carries all three so the shell's
/// device thread (which owns the !Send cpal streams) stays the single
/// forwarding point and no second signalling path can race the stop.
pub enum Control {
    Stop,
    Pause,
    Resume,
}

pub struct SourceInput {
    pub name: String,
    pub rate: u32,
    pub channels: u16,
    pub rx: Receiver<SourceMsg>,
}

pub struct SessionParams {
    pub dir: PathBuf,
    pub base: String,
    pub part: PathBuf,
    pub bitrate_kbps: u32,
    pub vault_name: String,
    pub recording_type: String,
    pub create_note: bool,
    pub recorded_at: String,
    pub flush_every: Duration,
    pub fsync_every: Duration,
    /// Live source-loss warnings, delivered while the recording continues.
    pub warn_tx: Option<Sender<String>>,
    /// Advisory live level meter: post-mix per-tick peak (0–1), sent every
    /// other tick (~5 Hz). Lossy by design — a gone receiver must never
    /// slow or fail the encode path.
    pub level_tx: Option<Sender<f32>>,
    /// Warning that predates the session (e.g. a configured device missing
    /// at start): seeds the worker's warning so it reaches the note's
    /// event metadata and the final Outcome exactly like a live warning.
    pub start_warning: Option<String>,
}

pub struct Outcome {
    pub mp3: PathBuf,
    pub note: Option<PathBuf>,
    pub duration_secs: u64,
    /// Total time spent paused (excluded from duration_secs and from the
    /// encoded timeline).
    pub paused_secs: u64,
    pub warning: Option<String>,
    pub ended_early: bool,
}

pub struct CaptureSession {
    control_tx: Sender<Control>,
    handle: JoinHandle<Result<Outcome, String>>,
}

const TARGET_RATE: u32 = 44_100;
/// Max buffered audio per source before oldest samples are dropped (2 s).
const BUFFER_CAP: usize = (TARGET_RATE * 2) as usize;
const TICK: Duration = Duration::from_millis(100);

struct SourceState {
    input: SourceInput,
    buffer: Vec<f32>, // mono @ TARGET_RATE
    alive: bool,
}

impl CaptureSession {
    pub fn start(
        params: SessionParams,
        sources: Vec<SourceInput>,
    ) -> std::io::Result<CaptureSession> {
        // Encoder init comes FIRST: a LAME setup failure must fail the
        // start before any file exists and before ready is signaled —
        // never a brief "recording" state that immediately dies.
        let encoder =
            Mp3Encoder::new(TARGET_RATE, params.bitrate_kbps).map_err(std::io::Error::other)?;
        // Exclusive create: an existing file here means an unrecovered
        // orphan won the name despite the reservation — never truncate it.
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&params.part)?;
        let (control_tx, control_rx) = mpsc::channel();
        let handle =
            std::thread::spawn(move || run_worker(file, encoder, params, sources, control_rx));
        Ok(CaptureSession { control_tx, handle })
    }

    pub fn is_running(&self) -> bool {
        !self.handle.is_finished()
    }

    /// Fire-and-forget: a dead worker (already finalizing) makes these
    /// no-ops, which is exactly right — pause must never block or fail
    /// shutdown.
    pub fn pause(&self) {
        let _ = self.control_tx.send(Control::Pause);
    }

    pub fn resume(&self) {
        let _ = self.control_tx.send(Control::Resume);
    }

    pub fn stop(self) -> Result<Outcome, String> {
        let _ = self.control_tx.send(Control::Stop);
        self.handle
            .join()
            .map_err(|_| "capture worker panicked".to_string())?
    }
}

fn run_worker(
    mut file: std::fs::File,
    mut encoder: Mp3Encoder,
    params: SessionParams,
    sources: Vec<SourceInput>,
    control_rx: Receiver<Control>,
) -> Result<Outcome, String> {
    let mut states: Vec<SourceState> = sources
        .into_iter()
        .map(|input| SourceState {
            input,
            buffer: Vec::new(),
            alive: true,
        })
        .collect();
    let device_names: Vec<String> = states.iter().map(|s| s.input.name.clone()).collect();
    let mut last_flush = Instant::now();
    let mut last_fsync = Instant::now();
    let mut frames_written: u64 = 0;
    let mut warning: Option<String> = params.start_warning.clone();
    let mut ended_early = false;
    // Set when an encode/write/flush call fails mid-recording. Rather than
    // returning immediately (which would abandon the .part file), we break
    // out of the loop and fall through to the normal finalize path so
    // whatever audio already made it into the encoder is saved best-effort
    // and surfaced to the caller instead of stranded as a hidden .part.
    let mut write_error: Option<String> = None;
    // Reusable per-tick buffers: one silence-padded chunk per source plus
    // a shared all-zeros pad for a missing side, kept across ticks so the
    // worker doesn't allocate fresh Vecs every 100 ms for hours.
    let mut chunks: Vec<Vec<f32>> = vec![Vec::new(); states.len()];
    let mut silence: Vec<f32> = Vec::new();

    // Tick on a fixed schedule rather than sleeping a full TICK per cycle:
    // each iteration consumes one TICK of audio, so waiting TICK *plus*
    // processing time would consume structurally below real time, filling
    // the buffers to BUFFER_CAP after a few minutes and then dropping
    // samples for the rest of a long recording. When processing overruns,
    // `wait` collapses to zero (recv_timeout returns immediately) and
    // catch-up cycles run back-to-back until the schedule is met again, so
    // average consumption matches real time; the buffer-occupancy drop then
    // only handles true device clock drift.
    let mut next_tick = Instant::now() + TICK;
    let mut paused = false;
    let mut paused_total = Duration::ZERO;
    let mut pause_started: Option<Instant> = None;
    let mut level_tick: u32 = 0;
    loop {
        let wait = next_tick.saturating_duration_since(Instant::now());
        let mut stopped = false;
        match control_rx.recv_timeout(wait) {
            Ok(Control::Stop) | Err(RecvTimeoutError::Disconnected) => stopped = true,
            Ok(Control::Pause) => {
                if !paused {
                    paused = true;
                    pause_started = Some(Instant::now());
                }
            }
            Ok(Control::Resume) => {
                if paused {
                    paused = false;
                    if let Some(started) = pause_started.take() {
                        paused_total += started.elapsed();
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        next_tick += TICK;

        // Drain every source's channel into its (converted) buffer.
        for s in states.iter_mut().filter(|s| s.alive) {
            loop {
                match s.input.rx.try_recv() {
                    Ok(SourceMsg::Samples(raw)) => {
                        // Paused: keep draining (device loss stays
                        // detectable, channels never back up) but discard —
                        // the encoded timeline skips the gap entirely.
                        if paused {
                            continue;
                        }
                        let mono = mixer::downmix_to_mono(&raw, s.input.channels);
                        let mono = mixer::resample_linear(&mono, s.input.rate, TARGET_RATE);
                        s.buffer.extend(mono);
                        if s.buffer.len() > BUFFER_CAP {
                            let drop = s.buffer.len() - BUFFER_CAP;
                            log::warn!(
                                "capture: dropping {drop} overflowed samples ({})",
                                s.input.name
                            );
                            s.buffer.drain(..drop);
                        }
                    }
                    Ok(SourceMsg::Lost) => {
                        s.alive = false;
                        let msg = format!("source lost: {}", s.input.name);
                        log::warn!("capture: {msg}");
                        if let Some(tx) = &params.warn_tx {
                            let _ = tx.send(msg.clone());
                        }
                        warning = Some(msg);
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        s.alive = false;
                        let msg = format!("source lost: {}", s.input.name);
                        if let Some(tx) = &params.warn_tx {
                            let _ = tx.send(msg.clone());
                        }
                        warning = Some(msg);
                        break;
                    }
                }
            }
        }

        // Every opened source is one the mode requires (spec: source loss
        // is judged against the mode's sources) — a meeting survives on
        // either stream; a mic-only voice note ends when the mic dies.
        let no_source_left = states.iter().all(|s| !s.alive);
        let finish = stopped || no_source_left;
        if no_source_left && !stopped {
            ended_early = true;
        }

        // Pull one tick's worth of audio per source (silence-fill a
        // starved side only while its source is still alive and we keep
        // recording; on finish, take what's left without padding).
        let tick_frames = (TARGET_RATE / 10) as usize;
        let take = if finish {
            states.iter().map(|s| s.buffer.len()).max().unwrap_or(0)
        } else if paused {
            0
        } else {
            tick_frames
        };
        if take > 0 {
            // Mic + loopback only in this increment: a 3rd+ source would be
            // silently dropped from the mix by design.
            debug_assert!(states.len() <= 2, "mixer folds only two sources");
            if silence.len() < take {
                silence.resize(take, 0.0);
            }
            for (s, chunk) in states.iter_mut().zip(chunks.iter_mut()) {
                let n = take.min(s.buffer.len());
                chunk.clear();
                chunk.extend(s.buffer.drain(..n));
                chunk.resize(take, 0.0); // silence-fill underrun
            }
            let a = chunks
                .first()
                .map(|v| v.as_slice())
                .unwrap_or(&silence[..take]);
            let b = chunks
                .get(1)
                .map(|v| v.as_slice())
                .unwrap_or(&silence[..take]);
            let stereo = mixer::mix_to_stereo_i16(a, b);
            if let Some(level_tx) = &params.level_tx {
                level_tick = level_tick.wrapping_add(1);
                if level_tick.is_multiple_of(2) {
                    let peak = stereo
                        .iter()
                        .map(|s| (*s as f32 / i16::MAX as f32).abs())
                        .fold(0.0f32, f32::max);
                    let _ = level_tx.send(peak);
                }
            }
            frames_written += (stereo.len() / 2) as u64;
            let bytes = match encoder.encode(&stereo) {
                Ok(b) => b,
                Err(e) => {
                    log::error!("capture: encode failed mid-recording: {e}");
                    write_error = Some(e);
                    break;
                }
            };
            if let Err(e) = file.write_all(&bytes) {
                log::error!("capture: write failed mid-recording: {e}");
                write_error = Some(e.to_string());
                break;
            }
        }

        if last_flush.elapsed() >= params.flush_every {
            if let Err(e) = file.flush() {
                log::error!("capture: flush failed mid-recording: {e}");
                write_error = Some(e.to_string());
                break;
            }
            last_flush = Instant::now();
        }
        if last_fsync.elapsed() >= params.fsync_every {
            let _ = file.sync_data();
            last_fsync = Instant::now();
        }

        if finish {
            break;
        }
    }

    // Stop while paused: close the open span so the note records it and
    // pause can never block or distort shutdown.
    if let Some(started) = pause_started.take() {
        paused_total += started.elapsed();
    }

    // Finalize: flush encoder, fsync, stop-time reservation + rename retry.
    // On a prior write_error every step here is best-effort (attempt, log
    // on failure, keep going) — the goal is to save whatever audio already
    // reached the encoder rather than guarantee a perfect tail frame. Only
    // the rename below is still allowed to fail the whole call: if the
    // reserved name can't be claimed there is nothing left to surface.
    if let Some(err) = &write_error {
        match encoder.finish() {
            Ok(tail) => {
                if let Err(e) = file.write_all(&tail) {
                    log::error!("capture: best-effort tail write failed after {err}: {e}");
                }
            }
            Err(e) => {
                log::error!("capture: best-effort encoder finish failed after {err}: {e}");
            }
        }
        if let Err(e) = file.sync_all() {
            log::error!("capture: best-effort fsync failed after {err}: {e}");
        }
    } else {
        let tail = encoder.finish()?;
        file.write_all(&tail).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    drop(file);

    let duration_secs = frames_written / TARGET_RATE as u64;
    let (mp3, note_path) =
        crate::recovery::rename_into_reserved(&params.part, &params.dir, &params.base)?;
    // Make the rename's directory entry durable where the platform
    // supports it (Unix dir fsync; NTFS journaling covers Windows). Worst
    // case the fsynced .part entry survives instead and the next launch's
    // recovery finalizes it — no audio is lost either way.
    #[cfg(unix)]
    if let Ok(dir_handle) = std::fs::File::open(&params.dir) {
        let _ = dir_handle.sync_all();
    }
    log::info!("capture: saved {}", mp3.display());

    if let Some(err) = write_error {
        ended_early = true;
        warning = Some(match warning {
            Some(prior) => format!("{prior}; recording ended early: {err}"),
            None => format!("recording ended early: {err}"),
        });
    }

    let note = if params.create_note {
        let meta = NoteMeta {
            recorded_at: params.recorded_at.clone(),
            duration_secs,
            vault_name: params.vault_name.clone(),
            recording_type: params.recording_type.clone(),
            paused: (paused_total.as_secs() > 0)
                .then(|| vault_buddy_core::capture_note::format_duration(paused_total.as_secs())),
            input_devices: device_names,
            event: warning.clone(),
        };
        let mp3_name = mp3.file_name().unwrap_or_default().to_string_lossy();
        // Collision-safe: a user or sync client grabbing the reserved
        // note name after the rename must cost us a suffix, not the note.
        match write_note_collision_safe(&note_path, &render_note(&meta, &mp3_name)) {
            Ok(written) => Some(written),
            Err(e) => {
                log::warn!("capture: note write failed: {e}");
                None
            }
        }
    } else {
        None
    };

    Ok(Outcome {
        mp3,
        note,
        duration_secs,
        paused_secs: paused_total.as_secs(),
        warning,
        ended_early,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn sine_chunks(rate: u32, secs: f32) -> Vec<Vec<f32>> {
        let frames = (rate as f32 * secs) as usize;
        (0..frames)
            .map(|i| ((i as f32 / rate as f32) * 440.0 * std::f32::consts::TAU).sin() * 0.4)
            .collect::<Vec<f32>>()
            .chunks(rate as usize / 10)
            .map(|c| c.to_vec())
            .collect()
    }

    fn params(dir: &std::path::Path) -> SessionParams {
        let names = vault_buddy_core::capture_paths::reserve_names(dir, "b");
        SessionParams {
            dir: dir.to_path_buf(),
            base: names.base,
            part: names.part,
            bitrate_kbps: 128,
            vault_name: "Work".into(),
            recording_type: "Meeting".into(),
            create_note: true,
            recorded_at: "2026-07-04T14:05:00+02:00".into(),
            flush_every: Duration::from_millis(100),
            fsync_every: Duration::from_secs(30),
            warn_tx: None,
            level_tx: None,
            start_warning: None,
        }
    }

    #[test]
    fn records_mixes_and_finalizes_with_note() {
        let dir = tempfile::tempdir().unwrap();
        let (tx_a, rx_a) = mpsc::channel();
        let (tx_b, rx_b) = mpsc::channel();
        let session = CaptureSession::start(
            params(dir.path()),
            vec![
                SourceInput {
                    name: "mic".into(),
                    rate: 44_100,
                    channels: 1,
                    rx: rx_a,
                },
                SourceInput {
                    name: "loopback".into(),
                    rate: 44_100,
                    channels: 1,
                    rx: rx_b,
                },
            ],
        )
        .unwrap();
        assert!(dir.path().join(".b.mp3.part").exists(), "part created");
        for chunk in sine_chunks(44_100, 1.0) {
            tx_a.send(SourceMsg::Samples(chunk.clone())).unwrap();
            tx_b.send(SourceMsg::Samples(chunk)).unwrap();
        }
        std::thread::sleep(Duration::from_millis(400));
        let outcome = session.stop().unwrap();
        assert_eq!(outcome.mp3, dir.path().join("b.mp3"));
        assert!(outcome.mp3.exists());
        assert!(
            !dir.path().join(".b.mp3.part").exists(),
            "part renamed away"
        );
        let note = outcome.note.expect("note written");
        let text = std::fs::read_to_string(&note).unwrap();
        assert!(text.contains("![[b.mp3]]"));
        assert!(!outcome.ended_early);
        let bytes = std::fs::read(&outcome.mp3).unwrap();
        assert!(crate::recovery::has_mp3_frame(&bytes));
    }

    #[test]
    fn losing_the_only_required_source_finalizes_early() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let (warn_tx, warn_rx) = mpsc::channel();
        let mut p = params(dir.path());
        p.warn_tx = Some(warn_tx);
        let session = CaptureSession::start(
            p,
            vec![SourceInput {
                name: "mic".into(),
                rate: 44_100,
                channels: 1,
                rx,
            }],
        )
        .unwrap();
        tx.send(SourceMsg::Samples(vec![0.1f32; 4410])).unwrap();
        tx.send(SourceMsg::Lost).unwrap();
        // the warning must arrive live, not only inside the final Outcome
        let live = warn_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            live.contains("mic"),
            "live warning names the source: {live}"
        );
        // worker should self-finalize; stop() then just collects the outcome
        std::thread::sleep(Duration::from_millis(500));
        let outcome = session.stop().unwrap();
        assert!(outcome.ended_early);
        assert!(outcome.mp3.exists());
    }

    #[test]
    fn stop_time_collision_advances_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let session = CaptureSession::start(
            params(dir.path()),
            vec![SourceInput {
                name: "mic".into(),
                rate: 44_100,
                channels: 1,
                rx,
            }],
        )
        .unwrap();
        tx.send(SourceMsg::Samples(vec![0.1f32; 4410])).unwrap();
        // a sync client grabs the final name mid-recording
        std::fs::write(dir.path().join("b.mp3"), "intruder").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let outcome = session.stop().unwrap();
        assert_eq!(outcome.mp3, dir.path().join("b (2).mp3"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("b.mp3")).unwrap(),
            "intruder"
        );
    }

    #[test]
    fn pause_excludes_the_gap_and_the_note_records_it() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let session = CaptureSession::start(
            params(dir.path()),
            vec![SourceInput {
                name: "mic".into(),
                rate: 44_100,
                channels: 1,
                rx,
            }],
        )
        .unwrap();
        // ~0.4 s of real audio while recording
        for chunk in sine_chunks(44_100, 0.4) {
            tx.send(SourceMsg::Samples(chunk)).unwrap();
        }
        std::thread::sleep(Duration::from_millis(400));
        session.pause();
        // 2.2 s of wall time that must NOT appear in the duration; samples
        // arriving while paused are discarded (the gap is skipped, not
        // recorded as silence)
        std::thread::sleep(Duration::from_millis(200));
        for chunk in sine_chunks(44_100, 0.5) {
            tx.send(SourceMsg::Samples(chunk)).unwrap();
        }
        std::thread::sleep(Duration::from_millis(2_000));
        session.resume();
        std::thread::sleep(Duration::from_millis(300));
        let outcome = session.stop().unwrap();
        assert!(
            outcome.paused_secs >= 2,
            "paused span accumulated: {}",
            outcome.paused_secs
        );
        // active wall time is ~0.7 s + scheduling slack; if the paused span
        // leaked into the timeline this would be >= 2
        assert!(
            outcome.duration_secs < 2,
            "paused wall time excluded: {}",
            outcome.duration_secs
        );
        assert!(outcome.mp3.exists());
        let note = std::fs::read_to_string(outcome.note.expect("note written")).unwrap();
        assert!(note.contains("paused: \"0:0"), "paused metadata: {note}");
    }

    #[test]
    fn stop_while_paused_finalizes_and_closes_the_open_span() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let session = CaptureSession::start(
            params(dir.path()),
            vec![SourceInput {
                name: "mic".into(),
                rate: 44_100,
                channels: 1,
                rx,
            }],
        )
        .unwrap();
        tx.send(SourceMsg::Samples(vec![0.1f32; 4410])).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        session.pause();
        std::thread::sleep(Duration::from_millis(1_100));
        // pause never blocks shutdown: stop while paused saves normally
        let outcome = session.stop().unwrap();
        assert!(outcome.mp3.exists());
        assert!(
            outcome.paused_secs >= 1,
            "open pause span closed at stop: {}",
            outcome.paused_secs
        );
    }

    #[test]
    fn resume_without_pause_and_double_pause_are_harmless() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let session = CaptureSession::start(
            params(dir.path()),
            vec![SourceInput {
                name: "mic".into(),
                rate: 44_100,
                channels: 1,
                rx,
            }],
        )
        .unwrap();
        session.resume(); // no-op
        session.pause();
        session.pause(); // second pause must not restart the span
        std::thread::sleep(Duration::from_millis(200));
        session.resume();
        tx.send(SourceMsg::Samples(vec![0.1f32; 4410])).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let outcome = session.stop().unwrap();
        assert!(outcome.mp3.exists());
    }

    #[test]
    fn level_tap_reports_normalized_peaks_for_a_known_sine() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let (level_tx, level_rx) = mpsc::channel();
        let mut p = params(dir.path());
        p.level_tx = Some(level_tx);
        let session = CaptureSession::start(
            p,
            vec![SourceInput {
                name: "mic".into(),
                rate: 44_100,
                channels: 1,
                rx,
            }],
        )
        .unwrap();
        for chunk in sine_chunks(44_100, 1.0) {
            tx.send(SourceMsg::Samples(chunk)).unwrap();
        }
        std::thread::sleep(Duration::from_millis(600));
        let outcome = session.stop().unwrap();
        assert!(outcome.mp3.exists());
        let levels: Vec<f32> = level_rx.try_iter().collect();
        assert!(!levels.is_empty(), "levels were emitted");
        assert!(
            levels.iter().all(|l| (0.0..=1.0).contains(l)),
            "normalized 0-1: {levels:?}"
        );
        // 0.4-amplitude sine through soft_clip peaks near tanh(0.4) ≈ 0.38
        let max = levels.iter().cloned().fold(0.0f32, f32::max);
        assert!(max > 0.2, "peak tracks the signal: {max}");
    }

    #[test]
    fn part_creation_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".b.mp3.part"), "orphan").unwrap();
        let (_tx, rx) = mpsc::channel::<SourceMsg>();
        let mut p = params(dir.path());
        p.part = dir.path().join(".b.mp3.part"); // simulate racing reservation
        p.base = "b".into();
        let result = CaptureSession::start(
            p,
            vec![SourceInput {
                name: "mic".into(),
                rate: 44_100,
                channels: 1,
                rx,
            }],
        );
        assert!(result.is_err(), "must not truncate the orphan");
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".b.mp3.part")).unwrap(),
            "orphan"
        );
    }

    #[test]
    fn start_warning_reaches_outcome_and_note_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let mut p = params(dir.path());
        p.start_warning = Some("Configured microphone \"X\" not found".into());
        let session = CaptureSession::start(
            p,
            vec![SourceInput {
                name: "mic".into(),
                rate: 44_100,
                channels: 1,
                rx,
            }],
        )
        .unwrap();
        tx.send(SourceMsg::Samples(vec![0.1f32; 4410])).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let outcome = session.stop().unwrap();
        assert!(
            outcome
                .warning
                .as_deref()
                .unwrap_or("")
                .contains("not found"),
            "warning surfaced: {:?}",
            outcome.warning
        );
        assert!(!outcome.ended_early, "a fallback is not an early end");
        let note = std::fs::read_to_string(outcome.note.expect("note")).unwrap();
        assert!(note.contains("event:"), "note metadata event: {note}");
    }
}
