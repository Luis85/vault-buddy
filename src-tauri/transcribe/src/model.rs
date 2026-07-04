//! Whisper ggml model registry and on-disk cache. Models are downloaded on
//! first use (never bundled) from Hugging Face into %APPDATA%\vault-buddy\
//! models — the only network access in the app.

use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModelTier {
    Base,
    Small,
    Medium,
}

impl ModelTier {
    /// Infallible by design (unrecognized input defaults to `Small`), so this
    /// intentionally isn't `std::str::FromStr` — that trait's `from_str`
    /// returns a `Result`, which doesn't fit "always resolves to a tier."
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> ModelTier {
        match s {
            "base" => ModelTier::Base,
            "medium" => ModelTier::Medium,
            _ => ModelTier::Small, // small is the default tier
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelTier::Base => "base",
            ModelTier::Small => "small",
            ModelTier::Medium => "medium",
        }
    }
    /// Label recorded in transcript frontmatter.
    pub fn label(&self) -> String {
        format!("whisper-{}", self.as_str())
    }
    pub fn file_name(&self) -> &'static str {
        match self {
            ModelTier::Base => "ggml-base.bin",
            ModelTier::Small => "ggml-small.bin",
            ModelTier::Medium => "ggml-medium.bin",
        }
    }
    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
            self.file_name()
        )
    }
    /// A sanity floor (not a checksum): a downloaded file far below this is a
    /// partial/failed transfer. A corrupt-but-large file is caught when the
    /// engine fails to load it (retryable).
    pub fn min_size(&self) -> u64 {
        match self {
            ModelTier::Base => 100_000_000,     // ~142 MB
            ModelTier::Small => 300_000_000,    // ~466 MB
            ModelTier::Medium => 1_000_000_000, // ~1.5 GB
        }
    }
}

/// `%APPDATA%\vault-buddy\models` — app-side, never inside a vault.
pub fn model_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("vault-buddy").join("models"))
}

pub fn model_path(tier: ModelTier) -> Option<PathBuf> {
    model_dir().map(|d| d.join(tier.file_name()))
}

/// Download the tier's ggml model with progress, `.part`-then-rename. Skips
/// if already present. `on_progress(received, total)` is called per chunk.
pub fn download_model(
    tier: ModelTier,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<PathBuf, String> {
    let dir = model_dir().ok_or("cannot resolve model directory")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create model dir: {e}"))?;
    let dest = dir.join(tier.file_name());
    if dest.exists() {
        return Ok(dest);
    }
    let resp = ureq::get(&tier.url())
        .call()
        .map_err(|e| format!("request model: {e}"))?;
    let total: Option<u64> = resp.header("Content-Length").and_then(|v| v.parse().ok());
    let part = dir.join(format!("{}.part", tier.file_name()));
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&part).map_err(|e| format!("create model temp: {e}"))?;
    let mut buf = [0u8; 64 * 1024];
    let mut received: u64 = 0;
    loop {
        let n =
            std::io::Read::read(&mut reader, &mut buf).map_err(|e| format!("read stream: {e}"))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n]).map_err(|e| format!("write model: {e}"))?;
        received += n as u64;
        on_progress(received, total);
    }
    let _ = std::io::Write::flush(&mut file);
    let _ = file.sync_all();
    drop(file);
    if received < tier.min_size() {
        let _ = std::fs::remove_file(&part);
        return Err(format!("model download incomplete: {received} bytes"));
    }
    // We own `part` and `dest` didn't exist above — a plain rename is fine.
    std::fs::rename(&part, &dest).map_err(|e| format!("finalize model: {e}"))?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_from_str_defaults_to_small() {
        assert_eq!(ModelTier::from_str("base"), ModelTier::Base);
        assert_eq!(ModelTier::from_str("medium"), ModelTier::Medium);
        assert_eq!(ModelTier::from_str("small"), ModelTier::Small);
        assert_eq!(ModelTier::from_str("garbage"), ModelTier::Small);
    }

    #[test]
    fn tier_files_urls_and_labels() {
        assert_eq!(ModelTier::Small.file_name(), "ggml-small.bin");
        assert!(ModelTier::Small.url().ends_with("/ggml-small.bin"));
        assert!(ModelTier::Small
            .url()
            .starts_with("https://huggingface.co/ggerganov/whisper.cpp"));
        assert_eq!(ModelTier::Base.label(), "whisper-base");
        assert_eq!(ModelTier::Small.as_str(), "small");
    }

    #[test]
    fn model_path_ends_with_the_tier_file() {
        if let Some(p) = model_path(ModelTier::Small) {
            assert_eq!(p.file_name().unwrap().to_string_lossy(), "ggml-small.bin");
        }
    }
}
