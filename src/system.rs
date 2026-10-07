//! Where Watchtower lives on disk and how it plugs into Windows.
//!
//! Watchtower is one folder: the executables, `data\` with the database and `skill\watchtower\`
//! with the Skill for agents. `WATCHTOWER_HOME` moves the data folder elsewhere.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// The Skill for agents, embedded so the exe always carries the matching version.
const SKILL_FILES: &[(&str, &str)] = &[
    ("SKILL.md", include_str!("../skill/watchtower/SKILL.md")),
    (
        "references/cli.md",
        include_str!("../skill/watchtower/references/cli.md"),
    ),
    (
        "references/card-guide.md",
        include_str!("../skill/watchtower/references/card-guide.md"),
    ),
    (
        "references/portfolio-guide.md",
        include_str!("../skill/watchtower/references/portfolio-guide.md"),
    ),
];

pub fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("Watchtower.exe"))
}

pub fn app_dir() -> PathBuf {
    exe_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn gui_exe_path() -> PathBuf {
    app_dir().join(format!("Watchtower-UI{}", std::env::consts::EXE_SUFFIX))
}

pub fn data_dir() -> PathBuf {
    let dir = std::env::var_os("WATCHTOWER_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| app_dir().join("data"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("watchtower.db")
}

pub fn skill_dir() -> PathBuf {
    app_dir().join("skill").join("watchtower")
}

/// Write the embedded Skill next to the exe, touching only files whose content differs.
pub fn export_skill() -> std::io::Result<PathBuf> {
    let dir = skill_dir();
    for (rel, content) in SKILL_FILES {
        let path = dir.join(rel);
        if std::fs::read_to_string(&path).is_ok_and(|current| current == *content) {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
    }
    Ok(dir)
}

/// What the settings panel shows.
pub fn settings() -> Value {
    let skill = skill_dir();
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "exe": exe_path().to_string_lossy(),
        "app_dir": app_dir().to_string_lossy(),
        "data_dir": data_dir().to_string_lossy(),
        "skill_dir": skill.to_string_lossy(),
        "vbot_install": format!("vbot skill install \"{}\" --scope global", skill.to_string_lossy()),
        "autostart": autostart_enabled(),
        "agent_access": agent_access_enabled(),
    })
}

#[cfg(windows)]
mod win {
    use std::path::Path;

    use winreg::enums::{RegType, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::{RegKey, RegValue};

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN_VALUE: &str = "Watchtower";

    pub fn autostart_enabled() -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(RUN_KEY)
            .and_then(|k| k.get_value::<String, _>(RUN_VALUE))
            .is_ok()
    }

    pub fn set_autostart(on: bool, exe: &Path) -> std::io::Result<()> {
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?;
        if on {
            key.set_value(RUN_VALUE, &format!("\"{}\"", exe.display()))
        } else {
            match key.delete_value(RUN_VALUE) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
        }
    }

    fn norm(p: &str) -> String {
        p.trim().trim_end_matches(['\\', '/']).to_lowercase()
    }

    fn user_path() -> std::io::Result<(RegKey, String)> {
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)?;
        let path = key.get_value::<String, _>("Path").unwrap_or_default();
        Ok((key, path))
    }

    pub fn path_contains(dir: &Path) -> bool {
        let want = norm(&dir.to_string_lossy());
        user_path().is_ok_and(|(_, path)| path.split(';').any(|p| norm(p) == want))
    }

    pub fn set_path_entry(on: bool, dir: &Path) -> std::io::Result<()> {
        let (key, path) = user_path()?;
        let dir = dir.to_string_lossy().into_owned();
        let want = norm(&dir);
        let mut parts: Vec<&str> = path
            .split(';')
            .filter(|p| !p.trim().is_empty() && norm(p) != want)
            .collect();
        if on {
            parts.push(&dir);
        }
        let joined = parts.join(";");
        let bytes: Vec<u8> = joined
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        // REG_EXPAND_SZ keeps entries such as %USERPROFILE%\bin working.
        key.set_raw_value(
            "Path",
            &RegValue {
                vtype: RegType::REG_EXPAND_SZ,
                bytes,
            },
        )?;
        broadcast_environment_change();
        Ok(())
    }

    fn broadcast_environment_change() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
        };
        let name: Vec<u16> = "Environment".encode_utf16().chain([0]).collect();
        let mut result = 0usize;
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                name.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                3000,
                &mut result,
            );
        }
    }
}

pub fn autostart_enabled() -> bool {
    #[cfg(windows)]
    return win::autostart_enabled();
    #[cfg(not(windows))]
    false
}

/// Open Watchtower when the owner signs in to Windows.
pub fn set_autostart(on: bool) -> std::io::Result<()> {
    #[cfg(windows)]
    return win::set_autostart(on, &gui_exe_path());
    #[cfg(not(windows))]
    {
        let _ = on;
        Err(std::io::Error::other(
            "autostart is only available on Windows",
        ))
    }
}

/// Whether `watchtower` is on the owner's PATH, so agents can call it by name.
pub fn agent_access_enabled() -> bool {
    #[cfg(windows)]
    return win::path_contains(&app_dir());
    #[cfg(not(windows))]
    false
}

pub fn set_agent_access(on: bool) -> std::io::Result<()> {
    #[cfg(windows)]
    return win::set_path_entry(on, &app_dir());
    #[cfg(not(windows))]
    {
        let _ = on;
        Err(std::io::Error::other(
            "PATH changes are only available on Windows",
        ))
    }
}
