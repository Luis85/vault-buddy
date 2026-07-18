//! Additive-template primitives shared by the note/task/document renderers.
//! `substitute` fills `{{token}}` placeholders; `sanitize_extra_frontmatter`
//! makes a user's extra-frontmatter text safe to inject before a closing
//! `---` — it can never break the fence or redefine a managed key.

/// Replace every `{{key}}` (whitespace inside the braces tolerated) with its
/// value from `vars`. An unknown key renders empty (the available keys are
/// documented in the UI). Unclosed `{{` is emitted literally. UTF-8 safe.
pub fn substitute(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find("}}") {
            let key = after[..end].trim();
            if let Some((_, val)) = vars.iter().find(|(k, _)| *k == key) {
                out.push_str(val);
            }
            rest = &after[end + 2..];
        } else {
            out.push_str("{{");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Return the lines of `text` safe to inject into a frontmatter block:
/// - a `---`/`...` line (a fence) is dropped, and so is any indented block
///   under it — user frontmatter can never break out of the block;
/// - a top-level line whose key (before the first `:`) is in `reserved`
///   (case-insensitive) is dropped along with its indented continuation
///   lines, so a managed key can't be redefined;
/// - blank lines are dropped.
///
/// Everything else is kept verbatim, newline-terminated.
pub fn sanitize_extra_frontmatter(text: &str, reserved: &[&str]) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            skipping = false;
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if !skipping {
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }
        if trimmed == "---" || trimmed == "..." {
            skipping = true;
            continue;
        }
        let key = line.split(':').next().unwrap_or("").trim();
        if reserved.iter().any(|r| r.eq_ignore_ascii_case(key)) {
            skipping = true;
            continue;
        }
        skipping = false;
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitute_fills_known_and_empties_unknown() {
        let vars = [("title", "Buy milk"), ("date", "2026-07-18")];
        assert_eq!(
            substitute("# {{title}} ({{date}})", &vars),
            "# Buy milk (2026-07-18)"
        );
        assert_eq!(substitute("{{ title }}", &vars), "Buy milk"); // whitespace tolerated
        assert_eq!(substitute("x {{nope}} y", &vars), "x  y"); // unknown → empty
    }

    #[test]
    fn substitute_is_utf8_safe_and_tolerates_unclosed() {
        assert_eq!(substitute("café {{x}}", &[]), "café ");
        assert_eq!(substitute("a {{ open", &[]), "a {{ open");
    }

    #[test]
    fn sanitize_drops_fences_and_reserved_keys() {
        let text = "project: Alpha\ntype: Evil\n---\nowner: me\ntags:\n  - x\n  - y\nnote: keep";
        let reserved = ["type", "tags"];
        let out = sanitize_extra_frontmatter(text, &reserved);
        assert!(out.contains("project: Alpha"));
        assert!(out.contains("note: keep"));
        assert!(!out.contains("type: Evil"), "reserved key dropped: {out}");
        assert!(!out.contains("---"), "fence dropped: {out}");
        assert!(!out.contains("- x"), "reserved block items dropped: {out}");
        // `owner: me` sits after a fence line → the fence starts a skip block,
        // but a following TOP-LEVEL key resets it and is kept.
        assert!(out.contains("owner: me"), "{out}");
    }

    #[test]
    fn sanitize_empty_in_empty_out() {
        assert_eq!(sanitize_extra_frontmatter("", &["type"]), "");
        assert_eq!(sanitize_extra_frontmatter("\n\n", &["type"]), "");
    }
}
