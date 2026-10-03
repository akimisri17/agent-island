//! Opens the person's terminal app in a folder. macOS uses `open -a`, which
//! needs no Automation permission; Windows uses Windows Terminal.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Preference order when the person has not picked one.
#[cfg(target_os = "macos")]
const KNOWN: [&str; 4] = ["Ghostty", "iTerm", "Warp", "Terminal"];

/// Terminal apps installed on this machine, in preference order.
#[cfg(target_os = "macos")]
pub fn installed() -> Vec<String> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let dirs = [PathBuf::from("/Applications"), home.join("Applications"), PathBuf::from("/System/Applications/Utilities")];
    KNOWN
        .iter()
        .filter(|name| dirs.iter().any(|d| d.join(format!("{name}.app")).exists()))
        .map(|s| s.to_string())
        .collect()
}

#[cfg(windows)]
pub fn installed() -> Vec<String> {
    let found = Command::new("where").arg("wt").output().is_ok_and(|o| o.status.success());
    if found { vec!["Windows Terminal".into()] } else { Vec::new() }
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn installed() -> Vec<String> {
    Vec::new()
}

/// The terminal to use: the person's pick if still installed, otherwise the
/// first installed one.
pub fn choose(pick: Option<&str>, installed: &[String]) -> Option<String> {
    pick.filter(|p| installed.iter().any(|i| i == p)).map(str::to_string).or_else(|| installed.first().cloned())
}

/// Program and arguments that open `app` in `dir`. No shell is involved, so
/// the folder name is passed through as one argument whatever it contains.
pub fn launch(app: &str, dir: &Path) -> (String, Vec<String>) {
    let d = dir.to_string_lossy().into_owned();
    if cfg!(windows) {
        ("wt".into(), vec!["-d".into(), d])
    } else {
        ("open".into(), vec!["-a".into(), app.into(), d])
    }
}

/// Opens the chosen terminal in `dir`.
pub fn open(pick: Option<&str>, dir: &Path) -> Result<(), String> {
    if !dir.is_absolute() || !dir.is_dir() {
        return Err("That folder no longer exists.".into());
    }
    let app = choose(pick, &installed()).ok_or("No supported terminal app found.")?;
    let (prog, args) = launch(&app, dir);
    let mut cmd = Command::new(&prog);
    cmd.args(&args);
    if cfg!(windows) {
        // Windows Terminal stays running; don't hold a thread waiting on it.
        cmd.spawn().map(|_| ()).map_err(|e| format!("Could not open {app}: {e}"))
    } else {
        let ok = cmd.status().map_err(|e| format!("Could not open {app}: {e}"))?;
        if ok.success() { Ok(()) } else { Err(format!("Could not open {app}.")) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_command_for_each_platform() {
        let dir = std::path::Path::new("/w/my shop");
        if cfg!(target_os = "macos") {
            assert_eq!(launch("iTerm", dir), ("open".to_string(), vec!["-a".into(), "iTerm".into(), "/w/my shop".into()]));
        } else if cfg!(windows) {
            assert_eq!(launch("Windows Terminal", dir), ("wt".to_string(), vec!["-d".into(), "/w/my shop".into()]));
        }
    }

    #[test]
    fn choice_falls_back_to_the_first_installed() {
        let installed = vec!["Ghostty".to_string(), "Terminal".to_string()];
        assert_eq!(choose(Some("Terminal"), &installed), Some("Terminal".to_string()));
        assert_eq!(choose(Some("Warp"), &installed), Some("Ghostty".to_string()), "a removed app falls back");
        assert_eq!(choose(None, &installed), Some("Ghostty".to_string()));
        assert_eq!(choose(None, &[]), None);
    }

    #[test]
    fn relative_folders_are_rejected() {
        assert!(open(None, Path::new("relative/dir")).is_err());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_always_has_terminal() {
        assert!(installed().contains(&"Terminal".to_string()));
    }
}
