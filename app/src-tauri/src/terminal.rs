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

/// How to start a program in a new terminal tab.
#[derive(Debug)]
pub enum Launch {
    /// Run this program with these arguments (no shell).
    Exec(String, Vec<String>),
    /// Write `body` to an executable .command file and open it with `app`.
    Script { app: String, body: String },
}

/// Single-quotes a string for /bin/sh.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// How `app` runs `program args…` in `dir`. Ghostty takes the program on its
/// command line; Terminal and iTerm run a .command script; Warp cannot run a
/// script, so Terminal does it instead.
pub fn command_launch(app: &str, dir: &Path, program: &Path, args: &[String]) -> Launch {
    let d = dir.to_string_lossy().into_owned();
    let p = program.to_string_lossy().into_owned();
    if cfg!(windows) {
        let mut a = vec!["-d".to_string(), d, p];
        a.extend(args.iter().cloned());
        return Launch::Exec("wt".into(), a);
    }
    if app == "Ghostty" {
        let mut a = vec!["-na".to_string(), "Ghostty".into(), "--args".into(), format!("--working-directory={d}"), "-e".into(), p];
        a.extend(args.iter().cloned());
        return Launch::Exec("open".into(), a);
    }
    let quoted: Vec<String> = std::iter::once(sh_quote(&p)).chain(args.iter().map(|a| sh_quote(a))).collect();
    let body = format!("#!/bin/sh\ncd {} && exec {}\n", sh_quote(&d), quoted.join(" "));
    let opener = if app == "iTerm" { "iTerm" } else { "Terminal" };
    Launch::Script { app: opener.into(), body }
}

/// Opens a new terminal tab in `dir` running `program args…`. `name` names
/// the script file when one is needed (letters, digits and dashes only).
pub fn open_command(pick: Option<&str>, dir: &Path, program: &Path, args: &[String], name: &str) -> Result<(), String> {
    if !dir.is_absolute() || !dir.is_dir() {
        return Err("That folder no longer exists.".into());
    }
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("Bad script name.".into());
    }
    let app = choose(pick, &installed()).ok_or("No supported terminal app found.")?;
    match command_launch(&app, dir, program, args) {
        Launch::Exec(prog, a) => {
            let mut cmd = Command::new(&prog);
            cmd.args(&a);
            if cfg!(windows) {
                cmd.spawn().map(|_| ()).map_err(|e| format!("Could not open {app}: {e}"))
            } else {
                let ok = cmd.status().map_err(|e| format!("Could not open {app}: {e}"))?;
                if ok.success() { Ok(()) } else { Err(format!("Could not open {app}.")) }
            }
        }
        Launch::Script { app: opener, body } => {
            let dir = std::env::temp_dir().join("agent-island");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let script = dir.join(format!("{name}.command"));
            std::fs::write(&script, body).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
            }
            let ok = Command::new("open").args(["-a", &opener]).arg(&script).status().map_err(|e| format!("Could not open {opener}: {e}"))?;
            if ok.success() { Ok(()) } else { Err(format!("Could not open {opener}.")) }
        }
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

    #[test]
    fn shell_quoting_survives_any_folder_name() {
        assert_eq!(sh_quote("/w/my shop"), "'/w/my shop'");
        assert_eq!(sh_quote("/w/it's"), r"'/w/it'\''s'");
    }

    #[test]
    fn command_launch_per_app() {
        let dir = Path::new("/w/my shop");
        let prog = Path::new("/u/.local/bin/claude");
        let args = vec!["--resume".to_string(), "ab-12".to_string()];
        if cfg!(target_os = "macos") {
            match command_launch("Ghostty", dir, prog, &args) {
                Launch::Exec(p, a) => {
                    assert_eq!(p, "open");
                    assert_eq!(a, vec!["-na", "Ghostty", "--args", "--working-directory=/w/my shop", "-e", "/u/.local/bin/claude", "--resume", "ab-12"]);
                }
                Launch::Script { .. } => panic!("Ghostty runs the program directly"),
            }
            for app in ["Terminal", "iTerm", "Warp"] {
                match command_launch(app, dir, prog, &args) {
                    Launch::Script { app: opener, body } => {
                        assert_eq!(opener, if app == "iTerm" { "iTerm" } else { "Terminal" }, "Warp can't run a script; Terminal does");
                        assert_eq!(body, "#!/bin/sh\ncd '/w/my shop' && exec '/u/.local/bin/claude' '--resume' 'ab-12'\n");
                    }
                    Launch::Exec(..) => panic!("{app} runs a .command script"),
                }
            }
        } else if cfg!(windows) {
            match command_launch("Windows Terminal", dir, prog, &args) {
                Launch::Exec(p, a) => {
                    assert_eq!(p, "wt");
                    assert_eq!(a, vec!["-d", "/w/my shop", "/u/.local/bin/claude", "--resume", "ab-12"]);
                }
                Launch::Script { .. } => panic!(),
            }
        }
    }
}
