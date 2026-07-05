/// The fields of a single crash, pre-rendered by the caller so this stays a
/// pure string builder (no Tauri, no std panic types) — unit-testable on any
/// platform. The shell's panic hook fills these from `PanicHookInfo`.
pub struct CrashRecord<'a> {
    pub timestamp: &'a str,
    pub thread: &'a str,
    pub message: &'a str,
    pub location: Option<&'a str>,
    pub backtrace: &'a str,
    pub app_version: &'a str,
    /// e.g. "windows x86_64"
    pub os: &'a str,
}

/// Format one crash into a delimited, human-readable block. The leading
/// marker line makes successive crashes greppable in a single file, and the
/// trailing blank line separates appended records.
pub fn format_crash_record(record: &CrashRecord) -> String {
    let location = record.location.unwrap_or("<unknown location>");
    format!(
        "==== VAULT BUDDY PANIC {timestamp} ====\n\
         version: {app_version}\n\
         os: {os}\n\
         thread: {thread}\n\
         location: {location}\n\
         message: {message}\n\
         backtrace:\n{backtrace}\n\n",
        timestamp = record.timestamp,
        app_version = record.app_version,
        os = record.os,
        thread = record.thread,
        location = location,
        message = record.message,
        backtrace = record.backtrace,
    )
}

/// Render a `u32` as `0x` + 8 lowercase hex digits into a caller-owned stack
/// buffer — no `format!`, so it's safe to call from the native crash
/// handler, which must not touch the allocator at crash time (see
/// `install_native_crash_handler` in the shell crate). Returns the filled
/// slice of `out` (always all 10 bytes).
pub fn hex_u32(value: u32, out: &mut [u8; 10]) -> &[u8] {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    out[0] = b'0';
    out[1] = b'x';
    for i in 0..8 {
        let shift = (7 - i) * 4;
        let nibble = ((value >> shift) & 0xf) as usize;
        out[2 + i] = DIGITS[nibble];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_u32_formats_as_zero_padded_lowercase_hex() {
        let mut buf = [0u8; 10];
        assert_eq!(hex_u32(0xC0000005, &mut buf), b"0xc0000005");
        assert_eq!(hex_u32(0, &mut buf), b"0x00000000");
        assert_eq!(hex_u32(11, &mut buf), b"0x0000000b");
    }

    fn sample(location: Option<&'static str>) -> String {
        format_crash_record(&CrashRecord {
            timestamp: "2026-07-04 21:30:00.123 +0000",
            thread: "main",
            message: "called `Option::unwrap()` on a `None` value",
            location,
            backtrace: "0: some::frame",
            app_version: "1.2.3",
            os: "windows x86_64",
        })
    }

    #[test]
    fn includes_every_field_under_the_marker() {
        let out = sample(Some("src-tauri/src/commands.rs:16:5"));
        assert!(
            out.starts_with("==== VAULT BUDDY PANIC 2026-07-04"),
            "got: {out}"
        );
        assert!(out.contains("thread: main"));
        assert!(out.contains("location: src-tauri/src/commands.rs:16:5"));
        assert!(out.contains("called `Option::unwrap()` on a `None` value"));
        assert!(out.contains("backtrace:\n0: some::frame"));
        assert!(
            out.ends_with("\n\n"),
            "records must be blank-line separated"
        );
    }

    #[test]
    fn missing_location_degrades_to_a_placeholder() {
        let out = sample(None);
        assert!(out.contains("location: <unknown location>"), "got: {out}");
    }

    #[test]
    fn includes_app_version_and_os() {
        let out = sample(Some("src-tauri/src/commands.rs:16:5"));
        assert!(out.contains("version: 1.2.3"), "got: {out}");
        assert!(out.contains("os: windows x86_64"), "got: {out}");
    }
}
