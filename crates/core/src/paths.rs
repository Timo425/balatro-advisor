//! Finding the Balatro save directory.
//!
//! Order: explicit path → `BALATRO_DIR` → `save_dir` in
//! `~/.config/balatro-advisor/config.toml` → auto-discovery (Proton prefixes,
//! `%APPDATA%\Balatro`, macOS).

use std::path::{Path, PathBuf};

use crate::Error;

const APP_ID: &str = "2379780";
const PROTON_SUFFIX: &str = "pfx/drive_c/users/steamuser/AppData/Roaming/Balatro";

#[derive(Debug, Default, serde::Deserialize)]
pub struct Config {
    pub save_dir: Option<PathBuf>,
    pub profile: Option<u8>,
}

impl Config {
    pub fn load() -> Config {
        config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }
}

pub fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".config")))?;
    Some(base.join("balatro-advisor/config.toml"))
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

/// Directories tried during auto-discovery, most likely first.
pub fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        out.push(PathBuf::from(appdata).join("Balatro"));
    }
    if let Some(h) = home() {
        let steam_roots = [
            h.join(".steam/debian-installation"),
            h.join(".steam/steam"),
            h.join(".local/share/Steam"),
            h.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        ];
        let mut libraries: Vec<PathBuf> = Vec::new();
        for root in &steam_roots {
            libraries.push(root.clone());
            libraries.extend(library_folders(&root.join("steamapps/libraryfolders.vdf")));
        }
        for lib in libraries {
            let p = lib.join("steamapps/compatdata").join(APP_ID).join(PROTON_SUFFIX);
            if !out.contains(&p) {
                out.push(p);
            }
        }
        out.push(h.join("Library/Application Support/Balatro"));
    }
    out
}

/// `"path"  "/mnt/games/SteamLibrary"` lines from Steam's libraryfolders.vdf.
fn library_folders(vdf: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(vdf) else { return Vec::new() };
    text.lines()
        .filter_map(|l| {
            let mut parts = l.split('"').filter(|s| !s.trim().is_empty());
            if parts.next()? == "path" { parts.next().map(PathBuf::from) } else { None }
        })
        .collect()
}

/// A directory counts as the Balatro save dir if it has `settings.jkr` or a profile folder.
fn looks_like_save_dir(p: &Path) -> bool {
    p.join("settings.jkr").is_file() || p.join("1/profile.jkr").is_file()
}

pub fn resolve_save_dir(explicit: Option<&Path>) -> Result<PathBuf, Error> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    if let Some(p) = std::env::var_os("BALATRO_DIR") {
        return Ok(PathBuf::from(p));
    }
    if let Some(p) = Config::load().save_dir {
        return Ok(p);
    }
    let tried = candidates();
    tried
        .iter()
        .find(|p| looks_like_save_dir(p))
        .cloned()
        .ok_or(Error::SaveDirNotFound(tried))
}

/// The profile currently selected in the game (`settings.jkr` → `profile`), default 1.
pub fn active_profile(save_dir: &Path) -> u8 {
    crate::jkr::read(&save_dir.join("settings.jkr"))
        .ok()
        .and_then(|v| v.get("profile").int())
        .and_then(|p| u8::try_from(p).ok())
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_library_folders() {
        let dir = std::env::temp_dir().join(format!("bav-vdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let vdf = dir.join("libraryfolders.vdf");
        std::fs::write(&vdf, "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"/mnt/games/Steam\"\n\t\t\"label\"\t\t\"\"\n\t}\n}\n")
            .unwrap();
        assert_eq!(library_folders(&vdf), vec![PathBuf::from("/mnt/games/Steam")]);
        std::fs::remove_dir_all(dir).ok();
    }
}
