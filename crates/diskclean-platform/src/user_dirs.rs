//! User directories without shell execution or assumptions about localization.
use std::{
    env,
    path::{Path, PathBuf},
};

#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Profile)
            .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }
}

#[must_use]
pub fn downloads_dir(home: &Path, config: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let _ = config;
        known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Downloads)
            .unwrap_or_else(|| home.join("Downloads"))
    }
    #[cfg(not(windows))]
    {
        std::fs::read_to_string(config.join("user-dirs.dirs"))
            .ok()
            .and_then(|text| parse_downloads(&text, home))
            .unwrap_or_else(|| home.join("Downloads"))
    }
}

/// Actual system directories, including installations outside the C: drive.
#[must_use]
pub fn system_directories() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::{
            FOLDERID_ProgramData, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, FOLDERID_Windows,
        };
        [
            &FOLDERID_Windows,
            &FOLDERID_ProgramFiles,
            &FOLDERID_ProgramFilesX86,
            &FOLDERID_ProgramData,
        ]
        .into_iter()
        .filter_map(known_folder)
        .collect()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[cfg(not(windows))]
fn parse_downloads(text: &str, home: &Path) -> Option<PathBuf> {
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        if key.trim() != "XDG_DOWNLOAD_DIR" {
            continue;
        }
        let value = value.trim().strip_prefix('"')?.strip_suffix('"')?;
        let mut decoded = String::new();
        let mut chars = value.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                let next = chars.next()?;
                if !matches!(next, '\\' | '"' | '$' | '`') {
                    return None;
                }
                decoded.push(next);
            } else {
                decoded.push(ch);
            }
        }
        if decoded == "$HOME" || decoded == "${HOME}" {
            return Some(home.to_path_buf());
        }
        if let Some(relative) = decoded
            .strip_prefix("$HOME/")
            .or_else(|| decoded.strip_prefix("${HOME}/"))
        {
            return Some(home.join(relative));
        }
        let path = PathBuf::from(decoded);
        return path.is_absolute().then_some(path);
    }
    None
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    use std::{os::windows::ffi::OsStringExt, ptr};
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath},
    };
    let mut buffer = ptr::null_mut();
    // SAFETY: id is a static GUID; buffer receives a COM-allocated UTF-16
    // NUL-terminated string, copied before being freed exactly once.
    unsafe {
        if SHGetKnownFolderPath(id, KF_FLAG_DONT_VERIFY as u32, ptr::null_mut(), &mut buffer) < 0 {
            return None;
        }
        if buffer.is_null() {
            return None;
        }
        let mut length = 0;
        while *buffer.add(length) != 0 {
            length += 1;
        }
        let value = std::ffi::OsString::from_wide(std::slice::from_raw_parts(buffer, length));
        CoTaskMemFree(buffer.cast());
        Some(PathBuf::from(value))
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;
    #[test]
    fn downloads_supports_localized_and_redirected_paths_without_shell_expansion() {
        let home = Path::new("/home/test");
        assert_eq!(
            parse_downloads("XDG_DOWNLOAD_DIR=\"$HOME/Загрузки\"", home),
            Some(home.join("Загрузки"))
        );
        assert_eq!(
            parse_downloads("XDG_DOWNLOAD_DIR=\"/data/files\"", home),
            Some("/data/files".into())
        );
        assert_eq!(
            parse_downloads("XDG_DOWNLOAD_DIR=\"$(touch bad)\"", home),
            None
        );
        assert_eq!(
            parse_downloads("XDG_DOWNLOAD_DIR=\"${HOME}\"", home),
            Some(home.to_path_buf())
        );
    }
}
