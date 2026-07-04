//! Obsidian liveness. obsidian.json keeps `open: true` on the vaults that
//! were open when Obsidian quit — that is how Obsidian knows what to restore
//! on the next launch — so the flag only means "open right now" while an
//! Obsidian process actually exists.

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

/// True when an Obsidian process is running on this machine.
pub fn obsidian_running() -> bool {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    sys.processes()
        .values()
        .any(|p| is_obsidian_process_name(&p.name().to_string_lossy()))
}

/// Matches the Obsidian executable across platforms — `Obsidian.exe` on
/// Windows, `obsidian` on Linux, `Obsidian` (and its Electron helpers,
/// e.g. "Obsidian Helper (Renderer)") on macOS.
pub fn is_obsidian_process_name(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("obsidian")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_obsidian_executable_on_every_platform() {
        assert!(is_obsidian_process_name("Obsidian.exe"));
        assert!(is_obsidian_process_name("obsidian"));
        assert!(is_obsidian_process_name("Obsidian"));
        assert!(is_obsidian_process_name("Obsidian Helper (Renderer)"));
    }

    #[test]
    fn does_not_match_other_processes() {
        assert!(!is_obsidian_process_name("vault-buddy"));
        assert!(!is_obsidian_process_name("explorer.exe"));
        // prefix match, not substring — a hypothetical "my-obsidian-sync"
        // daemon must not count as the app being open
        assert!(!is_obsidian_process_name("my-obsidian-sync"));
    }

    #[test]
    fn obsidian_running_probes_without_panicking() {
        // environment-dependent result — this pins down only that the
        // process scan itself works
        let _ = obsidian_running();
    }
}
