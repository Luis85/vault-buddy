//! Windows reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM0`-`COM9`,
//! `LPT0`-`LPT9`, and the superscript `COM¹²³`/`LPT¹²³`), for any caller in
//! this crate that turns untrusted text into a path component.
//!
//! Windows resolves a component whose STEM (the text before its first `.`,
//! trailing spaces ignored) is one of these to the device, not to a file,
//! whatever the extension or the surrounding directory: `media\CON.mp4` and
//! `media\CON .mp4` both open the console. First needed by
//! `editor::package::validate_entry_name` (Task 38): an asset id may be
//! `CON`, and a package names its media after asset ids.
//!
//! `screen::staging::is_reserved_device_stem` is an older twin in a crate
//! `core` cannot depend on. It covers `COM1`-`COM9`/`LPT1`-`LPT9` only and
//! does not trim trailing spaces; this one is deliberately the wider list
//! (Microsoft's "Naming Files" page also names `COM0`, `LPT0` and the
//! superscript digits), since refusing one of these costs nothing a real
//! package needs.

/// Whether one path COMPONENT (not a whole path) names a Windows device.
pub fn is_reserved_device_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component);
    let stem = stem.trim_end_matches(' ').to_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let Some(digit) = stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
    else {
        return false;
    };
    let mut chars = digit.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some('0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Regression (Task 38 fix round 1): a package may name its media after
    // an asset id, and `CON`/`NUL`/`COM1` are valid ids -- but on Windows
    // `media\CON.mp4` is the console device, not a file.
    #[test]
    fn reserved_device_names_match_case_insensitively_whatever_follows_the_stem() {
        for reserved in [
            "CON",
            "con",
            "Con.mp4",
            "nul.wav",
            "PRN",
            "aux",
            "COM1",
            "com9.mp4",
            "LPT1",
            "lpt9.json",
            "COM0",
            "LPT0",
            "CON.",
            "CON..",
            "CON .mp4",
            "NUL   ",
            "AUX. .x",
            "COM\u{b9}",
            "lpt\u{b3}.mp4",
            "CON.tar.gz",
        ] {
            assert!(
                is_reserved_device_name(reserved),
                "{reserved:?} is a device"
            );
        }
        for ordinary in [
            "COM10",
            "console",
            "CONx.mp4",
            "a1.mp4",
            "xCON",
            " CON",
            "LPT",
            "COM",
            "nul_1.mp4",
            "",
            ".CON",
        ] {
            assert!(!is_reserved_device_name(ordinary), "{ordinary:?} is a file");
        }
    }
}
