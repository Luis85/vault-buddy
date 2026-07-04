/// The fields of a single crash, pre-rendered by the caller so this stays a
/// pure string builder (no Tauri, no std panic types) — unit-testable on any
/// platform. The shell's panic hook fills these from `PanicHookInfo`.
pub struct CrashRecord<'a> {
    pub timestamp: &'a str,
    pub thread: &'a str,
    pub message: &'a str,
    pub location: Option<&'a str>,
    pub backtrace: &'a str,
}

/// Format one crash into a delimited, human-readable block. The leading
/// marker line makes successive crashes greppable in a single file, and the
/// trailing blank line separates appended records.
pub fn format_crash_record(record: &CrashRecord) -> String {
    let location = record.location.unwrap_or("<unknown location>");
    format!(
        "==== VAULT BUDDY PANIC {timestamp} ====\n\
         thread: {thread}\n\
         location: {location}\n\
         message: {message}\n\
         backtrace:\n{backtrace}\n\n",
        timestamp = record.timestamp,
        thread = record.thread,
        location = location,
        message = record.message,
        backtrace = record.backtrace,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(location: Option<&'static str>) -> String {
        format_crash_record(&CrashRecord {
            timestamp: "2026-07-04 21:30:00.123 +0000",
            thread: "main",
            message: "called `Option::unwrap()` on a `None` value",
            location,
            backtrace: "0: some::frame",
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
}
