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
}

pub struct Outcome {
    pub mp3: PathBuf,
    pub note: Option<PathBuf>,
    pub duration_secs: u64,
    pub warning: Option<String>,
    pub ended_early: bool,
}

pub struct CaptureSession {
    stop_tx: Sender<()>,
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
        // Exclusive create: an existing file here means an unrecovered
        // orphan won the name despite the reservation — never truncate it.
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&params.part)?;
        let (stop_tx, stop_rx) = mpsc::channel();
        let handle = std::thread::spawn(move || run_worker(file, params, sources, stop_rx));
        Ok(CaptureSession { stop_tx, handle })
    }

    pub fn is_running(&self) -> bool {
        !self.handle.is_finished()
    }

    pub fn stop(self) -> Result<Outcome, String> {
        let _ = self.stop_tx.send(());
        self.handle
            .join()
            .map_err(|_| "capture worker panicked".to_string())?
    }
}

fn run_worker(
    mut file: std::fs::File,
    params: SessionParams,
    sources: Vec<SourceInput>,
    stop_rx: Receiver<()>,
) -> Result<Outcome, String> {
    let mut encoder = match Mp3Encoder::new(TARGET_RATE, params.bitrate_kbps) {
        Ok(enc) => enc,
        Err(e) => {
            // Setup failed after the exclusive create — honor the
            // start-failure rule: no file left behind before the first
            // MP3 frame exists.
            drop(file);
            let _ = std::fs::remove_file(&params.part);
            return Err(e);
        }
    };
    let mut states: Vec<SourceState> = sources
        .into_iter()
        .map(|input| SourceState {
            input,
            buffer: Vec::new(),
            alive: true,
        })
        .collect();
    let device_names: Vec<String> = states.iter().map(|s| s.input.name.clone()).collect();
    let started = Instant::now();
    let mut last_flush = Instant::now();
    let mut last_fsync = Instant::now();
    let mut frames_written: u64 = 0;
    let mut warning: Option<String> = None;
    let mut ended_early = false;

    loop {
        let stopped = matches!(
            stop_rx.recv_timeout(TICK),
            Ok(()) | Err(RecvTimeoutError::Disconnected)
        );

        // Drain every source's channel into its (converted) buffer.
        for s in states.iter_mut().filter(|s| s.alive) {
            loop {
                match s.input.rx.try_recv() {
                    Ok(SourceMsg::Samples(raw)) => {
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
        } else {
            tick_frames
        };
        if take > 0 {
            let mut mono_slices: Vec<Vec<f32>> = Vec::with_capacity(states.len());
            for s in states.iter_mut() {
                let n = take.min(s.buffer.len());
                let mut chunk: Vec<f32> = s.buffer.drain(..n).collect();
                chunk.resize(take, 0.0); // silence-fill underrun
                mono_slices.push(chunk);
            }
            let silent = vec![0.0f32; take];
            let a = mono_slices.first().map(|v| v.as_slice()).unwrap_or(&silent);
            let b = mono_slices.get(1).map(|v| v.as_slice()).unwrap_or(&silent);
            let stereo = mixer::mix_to_stereo_i16(a, b);
            frames_written += (stereo.len() / 2) as u64;
            let bytes = encoder.encode(&stereo)?;
            file.write_all(&bytes).map_err(|e| e.to_string())?;
        }

        if last_flush.elapsed() >= params.flush_every {
            file.flush().map_err(|e| e.to_string())?;
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

    // Finalize: flush encoder, fsync, stop-time reservation + rename retry.
    let tail = encoder.finish()?;
    file.write_all(&tail).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);

    let duration_secs = frames_written / TARGET_RATE as u64;
    let _elapsed = started.elapsed();
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

    let note = if params.create_note {
        let meta = NoteMeta {
            recorded_at: params.recorded_at.clone(),
            duration_secs,
            vault_name: params.vault_name.clone(),
            recording_type: params.recording_type.clone(),
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
}
