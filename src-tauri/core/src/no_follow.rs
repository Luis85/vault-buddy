//! Opening one of OUR files for append without ever writing through a link
//! wearing its name (review finding S-9).
//!
//! A `symlink_metadata` check followed by `OpenOptions::open` is
//! check-then-open: between the two, something can swap a symlink (or any
//! other reparse point) in under the name, and the open then follows it
//! and appends the user's webcam bytes to whatever it points at. The check stays — it is the whole defence off Windows, where
//! this crate has no `O_NOFOLLOW` without a new dependency — but on Windows
//! the open itself is made no-follow: `FILE_FLAG_OPEN_REPARSE_POINT` opens a
//! reparse point AS ITSELF instead of its target, and the handle's own
//! attributes then say what was opened, so the verdict is about the very
//! object that would be written, with no window after it.
//!
//! In `core` rather than the shell for its test: this crate's suite runs on
//! the `windows-app` CI job (the shell's `--lib` suite runs on Linux only),
//! so the Windows arm's real-symlink test executes somewhere, not only on a
//! developer host that happens to hold the symlink privilege. (A JUNCTION
//! cannot stand in for the symlink there: `mklink /J` to a file makes a
//! directory entry, which an append open refuses with or without the flag —
//! probed while writing this, so no test pretends otherwise.)

use std::fs::{File, OpenOptions};
use std::path::Path;

/// Why `open_append_no_follow` refused.
#[derive(Debug)]
pub enum NoFollowError {
    /// The name is held by something other than a plain file (a symlink,
    /// any other reparse point, a directory). Nothing was written.
    NotAPlainFile,
    /// The open or the handle's metadata read failed.
    Io(std::io::Error),
}

/// `FILE_FLAG_OPEN_REPARSE_POINT` (winbase.h).
#[cfg(windows)]
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
/// `FILE_ATTRIBUTE_REPARSE_POINT` (winnt.h).
#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

/// Open the existing plain file at `path` for append, refusing — before a
/// byte is written — when the OPENED object is not a plain file.
pub fn open_append_no_follow(path: &Path) -> Result<File, NoFollowError> {
    let mut options = OpenOptions::new();
    options.append(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).map_err(NoFollowError::Io)?;
    let meta = file.metadata().map_err(NoFollowError::Io)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(NoFollowError::NotAPlainFile);
        }
    }
    if !meta.is_file() {
        return Err(NoFollowError::NotAPlainFile);
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn a_plain_file_opens_for_append() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join(".take.webm.part");
        std::fs::write(&part, b"ab").unwrap();
        let mut file = open_append_no_follow(&part).expect("a plain file opens");
        file.write_all(b"cd").unwrap();
        drop(file);
        assert_eq!(std::fs::read(&part).unwrap(), b"abcd");
    }

    #[test]
    fn a_missing_file_is_an_io_error_and_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join(".gone.webm.part");
        assert!(matches!(
            open_append_no_follow(&part),
            Err(NoFollowError::Io(_))
        ));
        assert!(!part.exists(), "an append open must never create the part");
    }

    // The Windows arm, on a real file symlink: the open must refuse the LINK
    // (not follow it to a plain target and pass), and the target is untouched.
    // Off Windows the open follows by design (the caller's no-follow
    // `symlink_metadata` check is the defence there), so this is Windows-only.
    #[cfg(windows)]
    #[test]
    fn a_symlink_wearing_the_name_is_refused_and_its_target_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let precious = dir.path().join("precious.bin");
        std::fs::write(&precious, b"not ours").unwrap();
        let link = dir.path().join(".take.webm.part");
        if let Err(e) = std::os::windows::fs::symlink_file(&precious, &link) {
            if e.raw_os_error() == Some(1314) {
                crate::services::test_announce::announce_skip(
                    "a_symlink_wearing_the_name_is_refused_and_its_target_untouched — \
                     symlink_file needs SeCreateSymbolicLinkPrivilege (Developer Mode or \
                     an elevated process); this account lacks it (OS error 1314)",
                );
                return;
            }
            panic!("symlink_file failed unexpectedly: {e}");
        }
        assert!(matches!(
            open_append_no_follow(&link),
            Err(NoFollowError::NotAPlainFile)
        ));
        assert_eq!(std::fs::read(&precious).unwrap(), b"not ours");
    }
}
