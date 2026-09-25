//! The tenth sanctioned vault write's landing (Task 48; ADR R13): copy an
//! immutable rendered product into a vault folder on the ninth write's
//! rails (`screen_capture_paths`).
//!
//! It differs from the ninth in exactly the two ways R13 names. It
//! **copies**: the bytes stream into an OWNED, hidden temp in the
//! destination directory (`create_owned_temp`, the note writer's marker),
//! and only then does `commit_screen_capture` land that temp -- pairwise
//! reservation, `rename_noreplace`, the ` (N)` retry, the note named from
//! where the video actually landed (A21). A same-directory move never falls
//! back to a copy, so the SOURCE is never opened for writing and never
//! removed: a product stays byte-identical and playable in its project. And
//! it deletes nothing else afterwards.
//!
//! **The final name never names a half-written file.** Until every byte is
//! fsync'd the file is `.<name>.mp4.vault-buddy.tmp`; a copy that fails or
//! is cancelled removes it, so a quit that cancels a publish leaves nothing
//! in the vault (the shutdown gate's publish term, `editor::publish`).

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::screen_capture_paths::{commit_screen_capture, create_owned_temp, mp4_file_name};

/// Bytes copied between two cancel checks.
const CHUNK: usize = 1024 * 1024;

/// Stream `source` into `dest` a chunk at a time, answering `cancel`
/// between chunks (`io::ErrorKind::Interrupted`) and reporting the fraction
/// of `total` written; fsync once every byte is across.
pub fn copy_cancellable(
    source: &mut File,
    dest: &mut File,
    total: u64,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(f64),
) -> io::Result<()> {
    let mut buffer = vec![0u8; CHUNK];
    let mut written = 0u64;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "the publish was cancelled",
            ));
        }
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        dest.write_all(&buffer[..read])?;
        written += read as u64;
        on_progress(if total == 0 {
            1.0
        } else {
            (written as f64 / total as f64).min(1.0)
        });
    }
    dest.sync_all()
}

/// Copy `from` into `dir` as `<base>.mp4` (or `<base> (N).mp4`), never
/// replacing anything; the landed video and its paired note path.
/// `stream` moves the bytes (`copy_cancellable` in production; the tests
/// observe or fail it). On any error the owned temp is gone.
pub fn copy_into_vault(
    from: &Path,
    dir: &Path,
    base: &str,
    stream: impl FnOnce(&mut File, &mut File) -> io::Result<()>,
) -> io::Result<(PathBuf, PathBuf)> {
    let mut source = File::open(from)?;
    let (tmp, mut dest) = create_owned_temp(&dir.join(mp4_file_name(base)))?;
    if let Err(e) = stream(&mut source, &mut dest) {
        drop(dest);
        remove_temp(&tmp);
        return Err(e);
    }
    drop(dest);
    // Same directory, so this is a `rename_noreplace` of OUR temp: the
    // product itself is not an argument here and cannot be moved.
    commit_screen_capture(&tmp, dir, base).map_err(|e| {
        remove_temp(&tmp);
        io::Error::other(e)
    })
}

/// Our own temp, gone on every failure path. A failed removal is logged:
/// the temp is hidden and marked, but nothing sweeps a vault folder.
fn remove_temp(tmp: &Path) {
    match std::fs::remove_file(tmp) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => log::warn!(
            "publish: could not remove the temporary file {:?}: {e}",
            tmp.file_name().unwrap_or_default()
        ),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn plain(from: &mut File, to: &mut File) -> io::Result<()> {
        io::copy(from, to).map(|_| ())
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    // R13: a COPY. The source is left exactly as it was, the landing is
    // pairwise and suffixed past an existing `.md` alone, and no temp is
    // left behind.
    #[test]
    fn a_copy_lands_pairwise_and_leaves_its_source() {
        let src = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let from = src.path().join("prod.mp4");
        fs::write(&from, b"rendered").unwrap();
        fs::write(vault.path().join("Demo.md"), b"theirs").unwrap();

        let (video, note) = copy_into_vault(&from, vault.path(), "Demo", plain).unwrap();

        assert_eq!(video, vault.path().join("Demo (2).mp4"));
        assert_eq!(note, vault.path().join("Demo (2).md"));
        assert_eq!(fs::read(&video).unwrap(), b"rendered");
        assert_eq!(fs::read(&from).unwrap(), b"rendered", "the source is kept");
        assert_eq!(names(vault.path()), ["Demo (2).mp4", "Demo.md"]);
    }

    // A cancel between chunks is an `Interrupted` error, and the half
    // written temp goes with it: nothing sweeps a vault folder.
    #[test]
    fn a_cancelled_copy_leaves_nothing_in_the_folder() {
        let src = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let from = src.path().join("prod.mp4");
        fs::write(&from, vec![7u8; CHUNK * 2 + 5]).unwrap();
        let cancel = AtomicBool::new(false);
        let mut seen = Vec::new();

        let e = copy_into_vault(&from, vault.path(), "Demo", |s, d| {
            copy_cancellable(s, d, (CHUNK * 2 + 5) as u64, &cancel, &mut |f| {
                seen.push(f);
                cancel.store(true, Ordering::SeqCst);
            })
        })
        .unwrap_err();

        assert_eq!(e.kind(), io::ErrorKind::Interrupted);
        assert_eq!(seen.len(), 1, "stopped at the first chunk boundary");
        assert!(names(vault.path()).is_empty(), "{:?}", names(vault.path()));
    }

    // Uncancelled, the fraction climbs to exactly 1 and every byte lands.
    #[test]
    fn a_full_copy_reports_its_progress_to_one() {
        let src = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let from = src.path().join("prod.mp4");
        let bytes: Vec<u8> = (0..CHUNK + 3).map(|i| (i % 251) as u8).collect();
        fs::write(&from, &bytes).unwrap();
        let mut seen = Vec::new();
        let (video, _) = copy_into_vault(&from, vault.path(), "Demo", |s, d| {
            copy_cancellable(
                s,
                d,
                bytes.len() as u64,
                &AtomicBool::new(false),
                &mut |f| seen.push(f),
            )
        })
        .unwrap();
        assert_eq!(fs::read(video).unwrap(), bytes);
        assert_eq!(seen.last().copied(), Some(1.0));
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{seen:?}");
    }
}
