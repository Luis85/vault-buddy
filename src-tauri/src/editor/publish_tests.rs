//! `publish.rs`' tests (Task 48): a real tempdir project store and session
//! and a real tempdir VAULT, with `PublishEnv` standing in for Obsidian's
//! registry -- and, where a test needs to, observing or failing the copy
//! and the note. What they pin is the tenth vault write's discipline:
//! never overwrite, video first and note second, the note naming the file
//! that actually landed (A21), nothing left behind by a failure, and the
//! product never touched.

use std::collections::BTreeMap;
use std::sync::mpsc;

use serde_json::json;
use vault_buddy_core::editor::{
    new_product, product_file_name, EditorSession, Map, Marker, Project, RenderRange,
};

use super::*;
use crate::editor::media_jobs::jobs_in;
use crate::editor::project_store::minimal_project;
use crate::editor::render_jobs::{render_blocks_shutdown, write_ledger};
use crate::editor::store_io::create_project;

const SESSION: &str = "ses-pub";
const PROJECT: &str = "proj-pub";
const PRODUCT: &str = "prod-a";
const VAULT: &str = "vault-1";
/// The ledger's `createdAt`: its OWN wall clock names the file.
const CREATED: &str = "2026-09-24T10:15:00+02:00";
/// `publish_base` of the fixture product.
const BASE: &str = "2026-09-24 1015 Walkthrough v2";
const BYTES: &[u8] = b"rendered product bytes";

type Hook<'a> = Box<dyn Fn(&AtomicBool) -> io::Result<()> + Sync + 'a>;

/// A tempdir vault. `hook` runs inside the copy, before the bytes move.
struct TestEnv<'a> {
    vault: PathBuf,
    free: Option<u64>,
    hook: Option<Hook<'a>>,
    fail_note: bool,
}

impl<'a> TestEnv<'a> {
    fn new(vault: &Path) -> Self {
        Self {
            vault: vault.to_path_buf(),
            free: None,
            hook: None,
            fail_note: false,
        }
    }

    fn with_hook(mut self, hook: impl Fn(&AtomicBool) -> io::Result<()> + Sync + 'a) -> Self {
        self.hook = Some(Box::new(hook));
        self
    }
}

impl PublishEnv for TestEnv<'_> {
    fn vault(&self, vault_id: &str) -> Option<(PathBuf, String)> {
        (vault_id == VAULT).then(|| (self.vault.clone(), "Engineering".to_string()))
    }

    fn config(&self, _vault_id: &str) -> VaultCaptureConfig {
        VaultCaptureConfig::default()
    }

    fn free_bytes(&self, _dir: &Path) -> Option<u64> {
        self.free
    }

    fn stream(
        &self,
        source: &mut File,
        dest: &mut File,
        total: u64,
        cancel: &AtomicBool,
        on_progress: &mut dyn FnMut(f64),
    ) -> io::Result<()> {
        if let Some(hook) = &self.hook {
            hook(cancel)?;
        }
        copy_cancellable(source, dest, total, cancel, on_progress)
    }

    fn write_note(&self, target: &Path, content: &str) -> io::Result<PathBuf> {
        if self.fail_note {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
        }
        write_note_collision_safe(target, content)
    }
}

fn marker(source_ms: u64, title: &str) -> Marker {
    Marker {
        id: format!("m-{source_ms}"),
        clip_id: "c1".into(),
        source_ms,
        title: title.into(),
        extra: Map::new(),
    }
}

/// The frozen edit the product was rendered from: one clip, one chapter.
fn snapshot() -> Project {
    let mut project: Project = serde_json::from_value(json!({
        "schema": "vault-buddy-video-project/3",
        "id": PROJECT,
        "title": "Walkthrough",
        "canvas": { "width": 1280, "height": 720, "fps": 30 },
        "master_gain": 1,
        "assets": [{ "id": "av", "kind": "video", "name": "demo.mp4", "duration_ms": 60000 }],
        "tracks": [{
            "id": "v1", "kind": "video", "name": "Screen", "visible": true,
            "locked": false, "muted": false, "solo": false, "volume": 1
        }],
        "clips": [{
            "id": "c1", "asset_id": "av", "track_id": "v1", "name": "c1",
            "start_ms": 0, "in_ms": 0, "out_ms": 60000,
            "fade_in_ms": 0, "fade_out_ms": 0, "fade_curve": "linear",
            "opacity": 1, "volume": 1, "muted": false,
            "x": 0, "y": 0, "w": 1, "h": 1
        }],
        "effects": [], "markers": [], "transitions": [],
        "destination": { "vault": VAULT, "folder": "", "dated": false }
    }))
    .unwrap();
    project.markers = vec![marker(7_000, "Intro")];
    project
}

/// A store with the project, its session and one rendered product.
fn store(root: &Path) -> EditorState {
    let state = EditorState::default();
    create_project(root, &minimal_project(PROJECT), &BTreeMap::new()).unwrap();
    let products = project_dir(root, PROJECT).unwrap().join(PRODUCTS_DIR);
    std::fs::create_dir_all(&products).unwrap();
    std::fs::write(products.join(product_file_name(PRODUCT)), BYTES).unwrap();
    let range = Some(RenderRange {
        start_ms: 5_000,
        end_ms: 60_000,
        extra: Map::new(),
    });
    let product = new_product(
        &snapshot(),
        9,
        PRODUCT,
        "Walkthrough v2",
        &product_file_name(PRODUCT),
        55_000,
        range,
        CREATED,
    );
    write_ledger(root, PROJECT, &[product]).unwrap();
    lock_ignoring_poison(&state.sessions).insert(
        SESSION.into(),
        EditorSession::resume(SESSION, minimal_project(PROJECT), 1),
    );
    lock_ignoring_poison(&state.by_project).insert(PROJECT.into(), SESSION.into());
    state
}

fn destination(folder: &str, dated: bool) -> PublishDestination {
    PublishDestination {
        vault_id: VAULT.into(),
        folder: folder.into(),
        dated,
        create_note: true,
    }
}

fn publish(
    state: &EditorState,
    root: &Path,
    env: &TestEnv<'_>,
    to: &PublishDestination,
) -> Result<PublishReceipt, EditorError> {
    publish_in(state, root, env, SESSION, PRODUCT, to)
}

/// Every file and directory under `dir`, relative, sorted.
fn tree(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            out.push(vault_relative(dir, &path));
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    out.sort();
    out
}

fn product_path(root: &Path) -> PathBuf {
    project_dir(root, PROJECT)
        .unwrap()
        .join(PRODUCTS_DIR)
        .join(product_file_name(PRODUCT))
}

// ---- the wire ----

#[test]
fn publish_wire_shapes_are_pinned() {
    let to: PublishDestination = serde_json::from_value(json!({
        "vaultId": "v", "folder": "Tutorials", "dated": true, "createNote": false
    }))
    .unwrap();
    assert_eq!(
        (
            to.vault_id.as_str(),
            to.folder.as_str(),
            to.dated,
            to.create_note
        ),
        ("v", "Tutorials", true, false)
    );
    let receipt = PublishReceipt {
        video_path: "C:\\v\\a.mp4".into(),
        note_path: None,
        vault_id: "v".into(),
        vault_name: "Notes".into(),
        warning: None,
    };
    assert_eq!(
        serde_json::to_value(&receipt).unwrap(),
        json!({
            "videoPath": "C:\\v\\a.mp4", "notePath": null, "vaultId": "v",
            "vaultName": "Notes", "warning": null
        })
    );
    let journal: PublishJournal = serde_json::from_value(json!({
        "step": "video", "video": "T/a.mp4", "note": "T/a.md"
    }))
    .unwrap();
    assert_eq!(journal.step, PublishStep::Video);
}

// ---- the tenth vault write ----

// Never overwrite: a same-named `.mp4` AND `.md` already in the folder are
// left byte-identical, and the pair lands under the next shared suffix.
#[test]
fn publish_never_overwrites() {
    let root = tempfile::tempdir().unwrap();
    let vault = tempfile::tempdir().unwrap();
    let state = store(root.path());
    let dir = vault.path().join("Tutorials");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{BASE}.mp4")), b"their video").unwrap();
    std::fs::write(dir.join(format!("{BASE}.md")), b"their note").unwrap();

    let receipt = publish(
        &state,
        root.path(),
        &TestEnv::new(vault.path()),
        &destination("Tutorials", false),
    )
    .unwrap();

    let video = dir.join(format!("{BASE} (2).mp4"));
    let note = dir.join(format!("{BASE} (2).md"));
    assert_eq!(receipt.video_path, video.to_string_lossy());
    assert_eq!(receipt.note_path, Some(note.to_string_lossy().into_owned()));
    assert_eq!(std::fs::read(&video).unwrap(), BYTES);
    assert_eq!(
        std::fs::read(dir.join(format!("{BASE}.mp4"))).unwrap(),
        b"their video"
    );
    assert_eq!(
        std::fs::read(dir.join(format!("{BASE}.md"))).unwrap(),
        b"their note"
    );
    assert_eq!(tree(&dir).len(), 4, "{:?}", tree(&dir));
    assert_eq!(
        (receipt.vault_id.as_str(), receipt.vault_name.as_str()),
        (VAULT, "Engineering")
    );
    assert_eq!(receipt.warning, None);
}

// A21: a file that appears DURING the copy (a sync client) takes the
// reserved name, so the commit retries onto ` (2)` -- and the note, named
// and rendered only after the video landed, says ` (2)` too. A note built
// from the reservation would embed a video that is not the one beside it.
#[test]
fn note_embeds_the_final_reserved_name() {
    let root = tempfile::tempdir().unwrap();
    let vault = tempfile::tempdir().unwrap();
    let state = store(root.path());
    let dir = vault.path().join("Tutorials");
    let env = TestEnv::new(vault.path())
        .with_hook(|_| std::fs::write(dir.join(format!("{BASE}.mp4")), b"synced in"));

    let receipt = publish(&state, root.path(), &env, &destination("Tutorials", false)).unwrap();

    let note_path = dir.join(format!("{BASE} (2).md"));
    assert_eq!(
        receipt.note_path,
        Some(note_path.to_string_lossy().into_owned())
    );
    let note = std::fs::read_to_string(&note_path).unwrap();
    assert!(note.contains(&format!("![[{BASE} (2).mp4]]")), "{note}");
    assert!(!note.contains(&format!("![[{BASE}.mp4]]")), "{note}");
    assert!(
        !dir.join(format!("{BASE}.md")).exists(),
        "no note for the reservation"
    );
    // The note is the product's: its range, its chapter at output time.
    assert!(note.contains("range: \"00:05-01:00\"\n"), "{note}");
    assert!(note.contains("- 00:02 Intro\n"), "{note}");
}

// Video first, note second: a note that cannot be written is a WARNING on
// a successful receipt, and the video stays.
#[test]
fn note_failure_keeps_the_video_and_warns() {
    let root = tempfile::tempdir().unwrap();
    let vault = tempfile::tempdir().unwrap();
    let state = store(root.path());
    let mut env = TestEnv::new(vault.path());
    env.fail_note = true;

    let receipt = publish(&state, root.path(), &env, &destination("Tutorials", false)).unwrap();

    assert_eq!(receipt.note_path, None);
    assert!(receipt.warning.as_deref().unwrap_or("").contains("note"));
    assert_eq!(std::fs::read(&receipt.video_path).unwrap(), BYTES);
    assert_eq!(
        tree(vault.path()),
        ["Tutorials", &format!("Tutorials/{BASE}.mp4")]
    );
}

// A copy that fails before the video lands (the disk fills) leaves the
// vault exactly as it found it: no temp, and none of the dated folders
// this publish created.
#[test]
fn failure_before_the_video_rolls_back_created_dirs() {
    let root = tempfile::tempdir().unwrap();
    let vault = tempfile::tempdir().unwrap();
    std::fs::write(vault.path().join("keep.md"), b"x").unwrap();
    let state = store(root.path());
    let env = TestEnv::new(vault.path())
        .with_hook(|_| Err(io::Error::new(io::ErrorKind::StorageFull, "disk full")));

    let e = publish(&state, root.path(), &env, &destination("Screens", true)).unwrap_err();

    assert_eq!(e.code, EditorErrorCode::DiskFull);
    assert_eq!(tree(vault.path()), ["keep.md"]);
    let jobs = jobs_in(&state, SESSION).unwrap();
    assert_eq!(jobs[0].phase, JobPhase::Failed);
    assert!(
        !project_dir(root.path(), PROJECT)
            .unwrap()
            .join(JOBS_DIR)
            .join(&jobs[0].job_id)
            .exists(),
        "the journal of a publish that returned is gone"
    );
}

// R13: the product is COPIED, never moved or opened for writing; the
// ledger is only read.
#[test]
fn publish_leaves_the_product_byte_identical() {
    let root = tempfile::tempdir().unwrap();
    let vault = tempfile::tempdir().unwrap();
    let state = store(root.path());
    let path = product_path(root.path());
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let ledger = project_dir(root.path(), PROJECT)
        .unwrap()
        .join("products.json");
    let ledger_before = std::fs::read(&ledger).unwrap();

    publish(
        &state,
        root.path(),
        &TestEnv::new(vault.path()),
        &destination("", true),
    )
    .unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), BYTES);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(std::fs::read(&ledger).unwrap(), ledger_before);
    // A blank folder is the vault's screen-capture folder, dated by the
    // product's own date.
    assert!(vault
        .path()
        .join("Screen Captures/2026/09")
        .join(format!("{BASE}.mp4"))
        .is_file());
}

// F19: the publish is a registered job BEFORE the video exists -- the
// registry reports `kind: "publish"` in a live phase during the copy, and
// the video's final name is not there yet.
#[test]
fn publish_registers_as_a_job_before_writing_the_video() {
    let root = tempfile::tempdir().unwrap();
    let vault = tempfile::tempdir().unwrap();
    let state = store(root.path());
    let dir = vault.path().join("Tutorials");
    let seen = std::sync::Mutex::new(Vec::new());
    let env = TestEnv::new(vault.path()).with_hook(|_| {
        let rows = jobs_in(&state, SESSION).unwrap();
        let wire = serde_json::to_value(&rows).unwrap();
        seen.lock()
            .unwrap()
            .push((wire, dir.join(format!("{BASE}.mp4")).exists()));
        Ok(())
    });

    publish(&state, root.path(), &env, &destination("Tutorials", false)).unwrap();
    drop(env);

    let seen = seen.into_inner().unwrap();
    let (rows, video_there) = &seen[0];
    assert_eq!(rows[0]["kind"], json!("publish"));
    assert_eq!(rows[0]["phase"], json!("publishing"));
    assert_eq!(rows[0]["terminal"], json!(null));
    assert!(
        !video_there,
        "the job was registered before the video was written"
    );
    let done = jobs_in(&state, SESSION).unwrap();
    assert_eq!(done[0].phase, JobPhase::Complete);
    assert_eq!(
        done[0].terminal.as_ref().unwrap().product_id.as_deref(),
        Some(PRODUCT)
    );
}

// F19 (controller ruling): the publish has its OWN shutdown term, by kind.
// While it copies, the publish term is true and the render term is not;
// the quit's bounded cancel ends it, and a cancelled copy leaves no temp
// and none of the folders it created.
#[test]
fn shutdown_is_blocked_while_publishing() {
    let root = tempfile::tempdir().unwrap();
    let vault = tempfile::tempdir().unwrap();
    let state = store(root.path());
    assert!(!publish_blocks_shutdown(&state));
    let (started, copying) = mpsc::channel();
    let env = TestEnv::new(vault.path()).with_hook(move |cancel| {
        started.send(()).unwrap();
        // Bounded, so a failing assertion below ends the test instead of
        // leaving this copy waiting on a cancel nobody sends.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !cancel.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
    });
    let outcome = std::thread::scope(|scope| {
        let worker =
            scope.spawn(|| publish(&state, root.path(), &env, &destination("Screens", true)));
        copying.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(
            publish_blocks_shutdown(&state),
            "a publish mid-copy blocks shutdown"
        );
        assert!(
            !render_blocks_shutdown(&state),
            "and is not reported as a render"
        );
        assert!(cancel_all_in(
            &state,
            Duration::from_secs(10),
            Duration::from_millis(5)
        ));
        assert!(!publish_blocks_shutdown(&state));
        worker.join().unwrap()
    });
    assert_eq!(outcome.unwrap_err().code, EditorErrorCode::Cancelled);
    assert!(tree(vault.path()).is_empty(), "{:?}", tree(vault.path()));
    assert_eq!(
        jobs_in(&state, SESSION).unwrap()[0].phase,
        JobPhase::Cancelled
    );
}

// Refusals come before anything exists: no vault, no job, no folder.
#[test]
fn a_refused_publish_creates_nothing() {
    let root = tempfile::tempdir().unwrap();
    let vault = tempfile::tempdir().unwrap();
    let state = store(root.path());
    let env = TestEnv::new(vault.path());
    let mut gone = destination("Tutorials", true);
    gone.vault_id = "vault-gone".into();
    assert_eq!(
        publish(&state, root.path(), &env, &gone).unwrap_err().code,
        EditorErrorCode::DestinationUnavailable
    );
    let outside =
        publish(&state, root.path(), &env, &destination("../outside", false)).unwrap_err();
    assert_eq!(outside.code, EditorErrorCode::DestinationUnavailable);
    // Final review M10: the folder is what the user typed; the refusal
    // says what is wrong with it without repeating it back.
    assert_eq!(
        outside.message,
        "That folder is outside the vault. Choose a folder inside it."
    );
    assert_eq!(
        publish_in(
            &state,
            root.path(),
            &env,
            SESSION,
            "prod-nope",
            &destination("", false)
        )
        .unwrap_err()
        .code,
        EditorErrorCode::InvalidRequest
    );
    std::fs::remove_file(product_path(root.path())).unwrap();
    assert_eq!(
        publish(&state, root.path(), &env, &destination("", false))
            .unwrap_err()
            .code,
        EditorErrorCode::SourceMissing
    );
    assert!(tree(vault.path()).is_empty());
    assert!(
        jobs_in(&state, SESSION).unwrap().is_empty(),
        "no job for a refusal"
    );
}

// Task 59 fix round 1 (review Minor 8): the export's structural rollback
// pin went with the export, and the tenth write rides the same rails. A
// failure after `prepare_export_dir` has CREATED folders in the user's vault
// must hand them to `rollback_export_dir`, or a refused publish leaves empty
// `Screen Captures/2026/09` litter behind (GAP-150's class). The behavioural
// test (`failure_before_the_video_rolls_back_created_dirs`) proves today's
// one fallible step; this walk -- the shared `assert_every_exit_is_paired`,
// which arms per BLOCK so a rollback on one branch never covers another --
// catches the NEXT fallible step added to `run_publish` without one.
#[test]
fn every_exit_after_the_folders_exist_rolls_them_back() {
    use crate::structural_scan::{assert_every_exit_is_paired, fn_body, production_half};

    let src = production_half(include_str!("publish.rs"));
    let body = fn_body(src, "fn run_publish(");
    let created = body
        .find("prepare_export_dir(")
        .expect("run_publish creates the vault folders");
    let eol = body[created..]
        .find('\n')
        .map(|i| created + i)
        .expect("its line ends");
    let scan =
        assert_every_exit_is_paired(&body[eol..], "rollback_export_dir(&created)", "run_publish");
    // Vacuity: a region with no exit and no rollback satisfies any rule.
    assert!(
        scan.exits >= 1 && scan.releases >= 1,
        "the walk found {} exit(s) and {} rollback(s) -- it is broken, not the invariant",
        scan.exits,
        scan.releases
    );
}

// F19, structural: both quit workers cancel a publish (bounded) before the
// two unbounded capture finalizes, beside the render cancel -- a copy left
// running behind them would keep writing into a vault for as long as they
// take. The hide chokepoint does not gate on it (the buddy is the
// RECORDING indicator; a publish has its own dialog).
#[test]
fn quit_cancels_a_publish_before_finalizing_captures() {
    use crate::structural_scan::{fn_body, offset_of, shell_file};

    let tray = shell_file("tray.rs");
    let close = shell_file("window_close.rs");
    for (name, body) in [
        ("tray::quit", fn_body(&tray, "pub fn quit(")),
        (
            "handle_main_close",
            fn_body(&close, "fn handle_main_close("),
        ),
    ] {
        let cancel = offset_of(body, "publish::cancel_all_bounded(");
        for later in ["finalize_if_recording(", "finalize_if_capturing("] {
            assert!(
                cancel < offset_of(body, later),
                "{name}: the publish must be cancelled before {later}"
            );
        }
    }
    assert!(!fn_body(&tray, "pub fn hide_buddy(").contains("publish::"));
}

// A discard removes the project directory the publish is READING the
// product from (and journaling into), so the publish must be stopped and
// ended before the removal -- its temp gone, the vault untouched -- not
// merely told to stop by the session's close afterwards.
#[test]
fn discarding_a_project_while_publishing_stops_the_publish_first() {
    use crate::editor::session_commands::{close_in, CloseDisposition};

    let root = tempfile::tempdir().unwrap();
    let vault = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let state = store(root.path());
    let ended = AtomicBool::new(false);
    let (started, copying) = mpsc::channel();
    let env = TestEnv::new(vault.path()).with_hook(move |cancel| {
        started.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !cancel.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
    });
    let outcome = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let outcome = publish(&state, root.path(), &env, &destination("Screens", true));
            ended.store(true, Ordering::SeqCst);
            outcome
        });
        copying.recv_timeout(Duration::from_secs(10)).unwrap();
        close_in(
            &state,
            root.path(),
            staging.path(),
            SESSION,
            CloseDisposition::DiscardProject,
        )
        .unwrap();
        assert!(
            !publish_blocks_shutdown(&state),
            "the publish was still running when the discard returned"
        );
        worker.join().unwrap()
    });
    assert!(ended.load(Ordering::SeqCst));
    assert_eq!(outcome.unwrap_err().code, EditorErrorCode::Cancelled);
    assert!(
        !project_dir(root.path(), PROJECT).unwrap().exists(),
        "the project is gone"
    );
    assert!(tree(vault.path()).is_empty(), "{:?}", tree(vault.path()));
}
