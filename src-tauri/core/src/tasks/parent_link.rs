//! Compose the `parent` link. A wikilink is used by default; a List folder can
//! legally contain wikilink metacharacters (`is_valid_list_name` rejects only
//! empty / `/` / `\` / a leading dot, and hand-created folders skip it), and
//! wikilinks have no escape for them — so those paths fall back to a
//! percent-encoded markdown link whose LABEL is also escaped.

use std::path::Path;

/// Characters that change a wikilink's meaning: `#` starts a heading target,
/// `|` an alias, `[`/`]` can terminate it, `^` a block ref.
const WIKILINK_UNSAFE: [char; 5] = ['#', '|', '[', ']', '^'];

/// `child_path` is the file the link will be WRITTEN INTO — needed only by the
/// markdown fallback, whose destination Obsidian resolves relative to the
/// containing note (design spec §1).
pub fn compose(
    parent_path: &Path,
    child_path: &Path,
    vault_root: &Path,
    parent_title: &str,
) -> Option<String> {
    let rel_no_ext = crate::uri::vault_relative_no_ext(parent_path, vault_root)?;
    if !rel_no_ext.contains(WIKILINK_UNSAFE) {
        // Wikilinks resolve by vault-wide name/path lookup, never relative to the
        // containing note — no child context needed.
        return Some(format!("[[{rel_no_ext}]]"));
    }
    // Fallback: a markdown destination is resolved FROM THE CHILD'S DIRECTORY, so
    // a vault-relative path would resolve as <child dir>/<vault path> — a dead
    // link. Emit a `../`-relative path instead; it resolves identically under
    // every Obsidian "new link format" setting, unlike a leading-slash form.
    let child_rel = crate::uri::vault_relative(child_path, vault_root)?;
    let child_dir_depth = child_rel.matches('/').count(); // segments above the file
    let mut dest = String::new();
    for _ in 0..child_dir_depth {
        dest.push_str("../");
    }
    dest.push_str(
        &rel_no_ext
            .split('/')
            .map(crate::uri::encode)
            .collect::<Vec<_>>()
            .join("/"),
    );
    Some(format!("[{}]({dest}.md)", escape_label(parent_title)))
}

/// Backslash-escape the characters that would break a markdown link label.
/// YAML quoting protects the surrounding scalar, not the Markdown parsed after
/// YAML decoding.
fn escape_label(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    for c in title.chars() {
        if matches!(c, '\\' | '[' | ']') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn an_ordinary_path_becomes_a_wikilink() {
        let root = PathBuf::from("/v");
        let p = PathBuf::from("/v/Tasks/Work/2026-07-04-ship.md");
        let c = PathBuf::from("/v/Tasks/Home/child.md");
        assert_eq!(
            compose(&p, &c, &root, "Ship it"),
            Some("[[Tasks/Work/2026-07-04-ship]]".to_string())
        );
    }

    #[test]
    fn a_metacharacter_list_falls_back_to_an_encoded_markdown_link() {
        // `Project#1` is a legal List folder; inside [[..]] the `#` would start a
        // heading target and silently point click-through at the wrong note.
        let root = PathBuf::from("/v");
        let p = PathBuf::from("/v/Tasks/Project#1/2026-07-04-ship.md");
        let c = PathBuf::from("/v/Tasks/child.md"); // one dir deep
        let link = compose(&p, &c, &root, "Ship it").unwrap();
        // `uri::encode` is `NON_ALPHANUMERIC`-based (see uri.rs's own
        // `builds_open_file_uri` test, which pins the same `-` -> `%2D`), so the
        // filename's hyphens are percent-encoded too, not just the `#`. That's
        // the established convention this composer deliberately reuses, per the
        // design spec: "the app already percent-encodes every `obsidian://`
        // parameter in `uri.rs`, so this reuses an established convention
        // rather than inventing one."
        assert_eq!(
            link,
            "[Ship it](../Tasks/Project%231/2026%2D07%2D04%2Dship.md)"
        );
        assert!(!link.starts_with("[[")); // not a wikilink
    }

    #[test]
    fn the_fallback_destination_is_relative_to_the_childs_directory() {
        // REGRESSION (design spec §1): a markdown destination resolves from the
        // note containing it, so a vault-relative path in a child under
        // Tasks/Work would resolve as Tasks/Work/Tasks/... — a dead link.
        let root = PathBuf::from("/v");
        let p = PathBuf::from("/v/Tasks/Project#1/t.md");
        let deep = PathBuf::from("/v/Tasks/Work/Sub/child.md"); // three dirs deep
        let link = compose(&p, &deep, &root, "T").unwrap();
        assert!(
            link.contains("](../../../Tasks/Project%231/t.md)"),
            "got {link}"
        );
        // A child at the vault root needs no ../ at all.
        let top = PathBuf::from("/v/child.md");
        let flat = compose(&p, &top, &root, "T").unwrap();
        assert!(flat.contains("](Tasks/Project%231/t.md)"), "got {flat}");
    }

    #[test]
    fn the_fallback_label_escapes_markdown_metacharacters() {
        // A title carrying `]` or `\` would otherwise produce a malformed link
        // even though the target is encoded.
        let root = PathBuf::from("/v");
        let p = PathBuf::from("/v/Tasks/A|B/t.md");
        let c = PathBuf::from("/v/Tasks/child.md");
        let link = compose(&p, &c, &root, r#"we [need] this \ now"#).unwrap();
        assert!(
            link.starts_with(r#"[we \[need\] this \\ now]("#),
            "got {link}"
        );
    }

    #[test]
    fn a_path_outside_the_vault_yields_none() {
        let c = PathBuf::from("/v/Tasks/child.md");
        assert_eq!(
            compose(Path::new("/other/t.md"), &c, Path::new("/v"), "T"),
            None
        );
    }
}
