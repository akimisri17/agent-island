//! The few choices a person can make, saved as JSON in the app's config folder.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_HOTKEY: &str = "ctrl+alt+KeyJ";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Global shortcut that jumps to the longest wait, e.g. "ctrl+alt+KeyJ".
    pub hotkey: String,
    /// Notify once per window when close to a usage limit, and on reset.
    pub notify_limits: bool,
    /// Allow the daily recap to be rewritten by the person's own `claude -p`.
    pub recap_with_claude: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { hotkey: DEFAULT_HOTKEY.into(), notify_limits: true, recap_with_claude: false }
    }
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.json")
}

/// Missing or unreadable settings fall back to defaults field by field.
pub fn load(config_dir: &Path) -> Settings {
    std::fs::read(path(config_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(config_dir: &Path, s: &Settings) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    let tmp = config_dir.join("settings.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(s)?)?;
    std::fs::rename(tmp, path(config_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let dir = std::env::temp_dir().join(format!("agent-island-settings-{}", std::process::id()));
        assert_eq!(load(&dir), Settings::default());
        let s = Settings { hotkey: "ctrl+shift+KeyK".into(), notify_limits: false, recap_with_claude: true };
        save(&dir, &s).unwrap();
        assert_eq!(load(&dir), s);
        // Unknown and missing fields are tolerated.
        std::fs::write(path(&dir), r#"{"hotkey":"alt+KeyW","future":1}"#).unwrap();
        let l = load(&dir);
        assert_eq!(l.hotkey, "alt+KeyW");
        assert!(l.notify_limits);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
