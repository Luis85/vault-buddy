//! Document Import: convert .docx/.odt/.rtf to a vault note via Pandoc.
//! Pure filename/frontmatter/path/staging logic; the shell drives Pandoc.
//! Fifth sanctioned vault write — same never-clobber discipline as the
//! capture note. Spec:
//! docs/superpowers/specs/2026-07-10-document-import-pandoc-design.md

use crate::capture_note::yaml_quote;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocFormat {
    Docx,
    Odt,
    Rtf,
}

impl DocFormat {
    /// Extension is authoritative (Obsidian/search treat extensions the same
    /// way). Case-insensitive so `.DOCX` from Windows still maps.
    pub fn from_extension(ext: &str) -> Option<DocFormat> {
        match ext.to_ascii_lowercase().as_str() {
            "docx" => Some(DocFormat::Docx),
            "odt" => Some(DocFormat::Odt),
            "rtf" => Some(DocFormat::Rtf),
            _ => None,
        }
    }

    /// The Pandoc `-f <reader>` value.
    pub fn reader(&self) -> &'static str {
        match self {
            DocFormat::Docx => "docx",
            DocFormat::Odt => "odt",
            DocFormat::Rtf => "rtf",
        }
    }

    /// Value written to the note's `format:` frontmatter field.
    pub fn label(&self) -> &'static str {
        self.reader()
    }
}

/// `YYYY-MM-DD <Original Name>` (no extension). `today` supplied by the shell
/// so the core stays clock-free.
pub fn document_basename(original_stem: &str, today: &str) -> String {
    format!("{today} {original_stem}")
}

pub struct DocMeta {
    /// The original file's absolute path (provenance).
    pub source_path: String,
    /// Import date, `YYYY-MM-DD`.
    pub imported: String,
    pub format: DocFormat,
}

/// The `type: Document` frontmatter block (no body — Pandoc's markdown is
/// prepended by the shell after this). Every string value quoted via
/// `yaml_quote`, so a Windows source path can't emit an invalid YAML escape.
pub fn render_frontmatter(meta: &DocMeta) -> String {
    format!(
        "---\ntype: Document\ntags: [vault-buddy-import]\nsource: {}\nimported: {}\nformat: {}\ncreated-by: Vault Buddy\n---\n\n",
        yaml_quote(&meta.source_path),
        yaml_quote(&meta.imported),
        yaml_quote(meta.format.label()),
    )
}

/// `<vault>/<documents_folder>/<YYYY>/<MM>`.
pub fn target_dir(vault_path: &Path, documents_folder: &str, year: &str, month: &str) -> PathBuf {
    vault_path.join(documents_folder).join(year).join(month)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn format_from_extension_is_case_insensitive_and_bounded() {
        assert_eq!(DocFormat::from_extension("docx"), Some(DocFormat::Docx));
        assert_eq!(DocFormat::from_extension("DOCX"), Some(DocFormat::Docx));
        assert_eq!(DocFormat::from_extension("odt"), Some(DocFormat::Odt));
        assert_eq!(DocFormat::from_extension("rtf"), Some(DocFormat::Rtf));
        assert_eq!(DocFormat::from_extension("pdf"), None);
        assert_eq!(DocFormat::Docx.reader(), "docx");
    }

    #[test]
    fn basename_is_date_prefixed_original_name() {
        assert_eq!(
            document_basename("Quarterly Report", "2026-07-10"),
            "2026-07-10 Quarterly Report"
        );
    }

    #[test]
    fn frontmatter_quotes_windows_source_path() {
        let meta = DocMeta {
            source_path: r"C:\Users\me\Quarterly Report.docx".into(),
            imported: "2026-07-10".into(),
            format: DocFormat::Docx,
        };
        let fm = render_frontmatter(&meta);
        assert!(fm.starts_with("---\n"));
        assert!(fm.contains("type: Document\n"));
        assert!(fm.contains("tags: [vault-buddy-import]\n"));
        // yaml_quote doubled the backslashes — no raw backslash escape in the scalar.
        assert!(fm.contains(r#"source: "C:\\Users\\me\\Quarterly Report.docx""#));
        // Every string value goes through yaml_quote — even closed-set ones — so
        // they land quoted (invariant: no bare scalars for string fields).
        assert!(fm.contains(r#"imported: "2026-07-10""#));
        assert!(fm.contains(r#"format: "docx""#));
        assert!(fm.trim_end().ends_with("---"));
    }

    #[test]
    fn target_dir_is_documents_folder_dated() {
        let d = target_dir(Path::new("/vault"), "Documents", "2026", "07");
        assert_eq!(d, Path::new("/vault/Documents/2026/07"));
    }
}
