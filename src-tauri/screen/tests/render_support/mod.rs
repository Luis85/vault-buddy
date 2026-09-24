//! Shared fixtures for `render_roundtrip.rs` (split out at the 800-line
//! cap, Task 45 fix round 1): the tools and what the installed ffmpeg can
//! do, the lavfi-synthesized inputs, and the readers that decode a render
//! back into pixels, band energies and durations.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tempfile::TempDir;
use vault_buddy_screen::render::{parse_filters_output, FfmpegCapabilities};

pub const W: u32 = 1280;
pub const H: u32 = 720;
pub const FPS: u32 = 30;
/// Every synthesized input is this long.
pub const INPUT_MS: u64 = 4_000;
/// The brief's duration tolerance for a range render.
pub const DURATION_TOLERANCE_MS: i64 = 40;
/// A band whose RMS is above this carries its tone; the other band of a
/// single-tone fixture measures about -60 dB (checked in test 3's control).
pub const BAND_PRESENT_DB: f64 = -40.0;

pub fn announce(message: &str) {
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr(), "{message}");
}

pub fn tool_on_path(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(&exe))
        .find(|candidate| candidate.is_file())
}

/// Everything one round trip needs: the tools, what the installed ffmpeg
/// can do, and a scratch directory that doubles as the render's job dir.
pub struct Fixture {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub caps: FfmpegCapabilities,
    pub dir: TempDir,
}

#[macro_export]
macro_rules! fixture_or_skip {
    () => {
        match $crate::render_support::fixture() {
            Some(fx) => fx,
            None => {
                let thread = std::thread::current();
                let who = thread.name().unwrap_or(module_path!()).to_string();
                $crate::render_support::announce(&format!(
                    "SKIPPED {who}: no ffmpeg/ffprobe on PATH, so this render round trip \
                     is UNPROVEN in this run. Install ffmpeg (CI does) to execute it."
                ));
                return;
            }
        }
    };
}

pub fn fixture() -> Option<Fixture> {
    let ffmpeg = tool_on_path("ffmpeg")?;
    let ffprobe = tool_on_path("ffprobe")?;
    let filters =
        String::from_utf8_lossy(&tool_stdout(&ffmpeg, &["-hide_banner", "-filters"])).into_owned();
    let encoders =
        String::from_utf8_lossy(&tool_stdout(&ffmpeg, &["-hide_banner", "-encoders"])).into_owned();
    let caps = parse_filters_output(&filters).with_encoders_output(&encoders);
    let dir = tempfile::tempdir().expect("tempdir");
    Some(Fixture {
        ffmpeg,
        ffprobe,
        caps,
        dir,
    })
}

pub fn tool_stdout<S: AsRef<std::ffi::OsStr>>(tool: &Path, args: &[S]) -> Vec<u8> {
    let out = Command::new(tool)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("the tool ran");
    assert!(
        out.status.success(),
        "{} failed: {}",
        tool.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

/// ffmpeg's stderr at `-v info` (where `astats` reports).
pub fn ffmpeg_stderr(ffmpeg: &Path, args: &[String]) -> String {
    let out = Command::new(ffmpeg)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("ffmpeg ran");
    assert!(
        out.status.success(),
        "ffmpeg {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

pub fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_string()).collect()
}

// ---------------------------------------------------------------- inputs

/// Encodes the lavfi `sources` (each `-f lavfi -i <spec>`) to `name`.
pub fn synthesize(fx: &Fixture, name: &str, sources: &[&str], codecs: &[&str]) -> PathBuf {
    let path = fx.dir.path().join(name);
    let mut args = strings(&["-v", "error"]);
    for spec in sources {
        args.extend(strings(&["-f", "lavfi", "-i", spec]));
    }
    args.extend(strings(codecs));
    args.push("-y".into());
    args.push(path.to_string_lossy().into_owned());
    tool_stdout(&fx.ffmpeg, &args);
    path
}

pub const VIDEO: [&str; 4] = ["-c:v", "libx264", "-pix_fmt", "yuv420p"];

/// A: `testsrc2` 1280x720 with a 440 Hz tone.
pub fn input_a(fx: &Fixture) -> PathBuf {
    let mut codecs = VIDEO.to_vec();
    codecs.extend(["-c:a", "aac"]);
    synthesize(
        fx,
        "a.mp4",
        &[
            "testsrc2=s=1280x720:r=30:d=4",
            "sine=frequency=440:sample_rate=48000:duration=4",
        ],
        &codecs,
    )
}

/// B: solid red 640x360, no audio.
pub fn input_b(fx: &Fixture) -> PathBuf {
    synthesize(fx, "b.mp4", &["color=c=red:s=640x360:r=30:d=4"], &VIDEO)
}

/// C: a 1000 Hz tone, no picture.
pub fn input_c(fx: &Fixture) -> PathBuf {
    synthesize(
        fx,
        "c.m4a",
        &["sine=frequency=1000:sample_rate=48000:duration=4"],
        &["-c:a", "aac"],
    )
}

/// D: 2 s of blue, then 2 s of green (lavfi `concat`).
pub fn input_d(fx: &Fixture) -> PathBuf {
    synthesize(
        fx,
        "d.mp4",
        &[
            "color=c=blue:s=1280x720:r=30:d=2[x];color=c=green:s=1280x720:r=30:d=2[y];\
           [x][y]concat=n=2:v=1:a=0",
        ],
        &VIDEO,
    )
}

/// E: plain black, for the text cue.
pub fn input_e(fx: &Fixture) -> PathBuf {
    synthesize(fx, "e.mp4", &["color=c=black:s=1280x720:r=30:d=4"], &VIDEO)
}

// ---------------------------------------------------------------- reading

pub type Rgb = (i32, i32, i32);

/// The whole RGB frame of `path` (at `w`x`h`) at `at_ms`.
pub fn frame_at(fx: &Fixture, path: &Path, at_ms: u64, (w, h): (u32, u32)) -> Vec<u8> {
    let mut args = strings(&["-v", "error", "-ss"]);
    args.push(format!("{}.{:03}", at_ms / 1000, at_ms % 1000));
    args.push("-i".into());
    args.push(path.to_string_lossy().into_owned());
    args.extend(strings(&[
        "-frames:v",
        "1",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgb24",
        "-",
    ]));
    let frame = tool_stdout(&fx.ffmpeg, &args);
    assert_eq!(
        frame.len(),
        (w * h * 3) as usize,
        "{} is not {w}x{h} at {at_ms} ms",
        path.display()
    );
    frame
}

pub fn canvas_frame(fx: &Fixture, path: &Path, at_ms: u64) -> Vec<u8> {
    frame_at(fx, path, at_ms, (W, H))
}

pub fn pixel(frame: &[u8], x: u32, y: u32) -> Rgb {
    let i = ((y * W + x) * 3) as usize;
    (frame[i] as i32, frame[i + 1] as i32, frame[i + 2] as i32)
}

pub fn is_red(p: Rgb) -> bool {
    p.0 > 200 && p.1 < 60 && p.2 < 60
}

pub fn is_green(p: Rgb) -> bool {
    p.0 < 60 && p.1 > 100 && p.2 < 60
}

pub fn is_blue(p: Rgb) -> bool {
    p.0 < 60 && p.1 < 60 && p.2 > 200
}

/// Mean of each channel over the rectangle `[x0,x1) x [y0,y1)`.
pub fn region_mean(frame: &[u8], (x0, y0, x1, y1): (u32, u32, u32, u32)) -> (f64, f64, f64) {
    let (mut r, mut g, mut b, mut n) = (0.0, 0.0, 0.0, 0.0);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = pixel(frame, x, y);
            r += f64::from(p.0);
            g += f64::from(p.1);
            b += f64::from(p.2);
            n += 1.0;
        }
    }
    (r / n, g / n, b / n)
}

/// Mean absolute per-byte difference of two equal-sized frames.
pub fn frame_distance(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let total: u64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    total as f64 / a.len() as f64
}

/// `astats`' overall RMS level (dB) of `path`'s audio over `[start, start +
/// len)` ms, through `bandpass` at `band` Hz (50 Hz wide) when given.
pub fn rms_db(fx: &Fixture, path: &Path, start_ms: u64, len_ms: u64, band: Option<u32>) -> f64 {
    let mut args = strings(&["-hide_banner", "-v", "info", "-ss"]);
    args.push(format!("{:.3}", start_ms as f64 / 1_000.0));
    args.push("-t".into());
    args.push(format!("{:.3}", len_ms as f64 / 1_000.0));
    args.push("-i".into());
    args.push(path.to_string_lossy().into_owned());
    args.push("-af".into());
    args.push(match band {
        Some(f) => format!("bandpass=f={f}:w=50,astats"),
        None => "astats".into(),
    });
    args.extend(strings(&["-f", "null", "-"]));
    let stderr = ffmpeg_stderr(&fx.ffmpeg, &args);
    // The LAST "RMS level dB" line is astats' "Overall" section.
    stderr
        .lines()
        .rev()
        .find_map(|l| l.split_once("RMS level dB: ").map(|(_, v)| v.trim()))
        .map(|v| {
            if v == "-inf" {
                f64::NEG_INFINITY
            } else {
                v.parse().expect("an RMS level")
            }
        })
        .unwrap_or_else(|| panic!("astats reported no RMS level: {stderr}"))
}

pub fn db_to_linear(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// The container duration of `path` in ms, read independently of the
/// render's own verification.
pub fn probe_duration_ms(fx: &Fixture, path: &Path) -> i64 {
    let mut args = strings(&[
        "-v",
        "error",
        "-show_entries",
        "format=duration",
        "-of",
        "csv=p=0",
    ]);
    args.push(path.to_string_lossy().into_owned());
    let text = String::from_utf8_lossy(&tool_stdout(&fx.ffprobe, &args)).into_owned();
    let seconds: f64 = text.trim().parse().expect("a duration");
    (seconds * 1_000.0).round() as i64
}

pub fn assert_duration_near(got_ms: i64, want_ms: i64, what: &str) {
    assert!(
        (got_ms - want_ms).abs() <= DURATION_TOLERANCE_MS,
        "{what}: the output is {got_ms} ms, want {want_ms} +/- {DURATION_TOLERANCE_MS} ms"
    );
}
