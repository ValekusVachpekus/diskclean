//! Read-only Windows Uninstall metadata. No command strings are read or executed.
use std::{io, path::PathBuf};

#[derive(Debug, Clone)]
pub struct RegistryApplication {
    /// Includes hive, registry view and subkey; never a filesystem path.
    pub key: String,
    pub name: String,
    pub version: Option<String>,
    pub install_path: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub struct RegistryInventory {
    pub applications: Vec<RegistryApplication>,
    pub issues: Vec<(String, String)>,
}

/// Missing/unsafe installer paths are retained as unknown, never guessed from
/// DisplayIcon, UninstallString or an estimated size. UNC/device paths and
/// environment expansion are deliberately unsupported.
pub fn installation_path(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(value);
    let bytes = value.as_bytes();
    if bytes.len() < 4
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || !matches!(bytes[2], b'\\' | b'/')
        || value.contains(['\0', '%', '"'])
        || value[2..].contains(':')
        || value[3..]
            .split(['\\', '/'])
            .any(|s| matches!(s, "." | ".."))
        || value[3..].split(['\\', '/']).all(str::is_empty)
    {
        return None;
    }
    Some(PathBuf::from(value))
}

pub fn discover(cancelled: impl Fn() -> bool) -> io::Result<RegistryInventory> {
    if cancelled() {
        return Err(io::ErrorKind::Interrupted.into());
    }
    #[cfg(windows)]
    {
        native::discover(cancelled)
    }
    #[cfg(not(windows))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows registry inventory is only available on Windows",
        ))
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod native {
    use super::*;
    use std::{collections::HashSet, ptr};
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
        System::Registry::*,
    };

    const UNINSTALL: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
    const MAX_KEYS: u32 = 20_000;
    const MAX_VALUE_BYTES: usize = 32 * 1024;
    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: this handle is owned, opened successfully, and closed once.
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
    fn status(code: u32) -> io::Result<()> {
        if code == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(code as i32))
        }
    }
    fn open(parent: HKEY, name: &str, view: u32) -> io::Result<Key> {
        let name = wide(name);
        let mut handle = ptr::null_mut();
        // SAFETY: the name is NUL-terminated and output handle points to live memory.
        // Only query/enumerate rights are requested. OPEN_LINK prevents following
        // the final registry link; link keys do not supply application values.
        status(unsafe {
            RegOpenKeyExW(
                parent,
                name.as_ptr(),
                REG_OPTION_OPEN_LINK,
                KEY_QUERY_VALUE | KEY_ENUMERATE_SUB_KEYS | view,
                &mut handle,
            )
        })?;
        Ok(Key(handle))
    }
    fn value(key: &Key, name: &str) -> io::Result<Option<(u32, Vec<u8>)>> {
        let name = wide(name);
        let mut data = vec![0u8; MAX_VALUE_BYTES];
        let mut len = data.len() as u32;
        let mut kind = 0;
        // SAFETY: the writable buffer has len bytes and all outputs are valid.
        // A fixed-size read bounds allocation even if the registry changes.
        let code = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                data.as_mut_ptr(),
                &mut len,
            )
        };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        status(code)?;
        data.truncate(len as usize);
        Ok(Some((kind, data)))
    }
    fn string(key: &Key, name: &str) -> io::Result<Option<String>> {
        let Some((kind, data)) = value(key, name)? else {
            return Ok(None);
        };
        if !matches!(kind, REG_SZ | REG_EXPAND_SZ) || data.len() % 2 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid registry string type/encoding",
            ));
        }
        let mut units: Vec<_> = data
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        if units.last() == Some(&0) {
            units.pop();
        }
        if units.contains(&0) {
            return Err(io::ErrorKind::InvalidData.into());
        }
        String::from_utf16(&units)
            .map(Some)
            .map_err(|_| io::ErrorKind::InvalidData.into())
    }
    fn application(key: &Key, reference: String) -> io::Result<Option<RegistryApplication>> {
        // Hidden system components stay out of the normal applications list.
        if let Some((kind, data)) = value(key, "SystemComponent")? {
            if kind != REG_DWORD || data.len() != 4 {
                return Err(io::ErrorKind::InvalidData.into());
            }
            if data.iter().any(|b| *b != 0) {
                return Ok(None);
            }
        }
        let Some(name) = string(key, "DisplayName")?.filter(|s| !s.trim().is_empty()) else {
            return Ok(None);
        };
        let version = string(key, "DisplayVersion")?.filter(|s| !s.trim().is_empty());
        let install_path = string(key, "InstallLocation")?.and_then(|s| installation_path(&s));
        Ok(Some(RegistryApplication {
            key: reference,
            name,
            version,
            install_path,
        }))
    }
    pub(super) fn discover(cancelled: impl Fn() -> bool) -> io::Result<RegistryInventory> {
        let mut inventory = RegistryInventory::default();
        let mut seen = HashSet::new();
        let mut total_keys = 0;
        let mut text_bytes = 0;
        for (hive, label) in [(HKEY_CURRENT_USER, "HKCU"), (HKEY_LOCAL_MACHINE, "HKLM")] {
            for (view, bits) in [(KEY_WOW64_64KEY, "64"), (KEY_WOW64_32KEY, "32")] {
                if cancelled() {
                    return Err(io::ErrorKind::Interrupted.into());
                }
                let reference = format!("{label}[{bits}]\\{UNINSTALL}");
                let root = match open(hive, UNINSTALL, view) {
                    Ok(root) => root,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(e) => {
                        inventory.issues.push((reference, e.to_string()));
                        continue;
                    }
                };
                for index in 0..=MAX_KEYS {
                    if cancelled() {
                        return Err(io::ErrorKind::Interrupted.into());
                    }
                    let mut name = [0u16; 256];
                    let mut len = name.len() as u32;
                    // SAFETY: a live key and writable UTF-16 buffer with its actual
                    // capacity; unused optional outputs are null.
                    let code = unsafe {
                        RegEnumKeyExW(
                            root.0,
                            index,
                            name.as_mut_ptr(),
                            &mut len,
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        )
                    };
                    if code == ERROR_NO_MORE_ITEMS {
                        break;
                    }
                    if total_keys == MAX_KEYS {
                        inventory
                            .issues
                            .push((reference.clone(), "registry entry limit reached".into()));
                        return Ok(inventory);
                    }
                    total_keys += 1;
                    if let Err(e) = status(code) {
                        inventory.issues.push((reference.clone(), e.to_string()));
                        continue;
                    }
                    let name = match String::from_utf16(&name[..len as usize]) {
                        Ok(name) => name,
                        Err(e) => {
                            inventory.issues.push((reference.clone(), e.to_string()));
                            continue;
                        }
                    };
                    let key_reference = format!("{reference}\\{name}");
                    match open(root.0, &name, view)
                        .and_then(|key| application(&key, key_reference.clone()))
                    {
                        Ok(Some(app)) => {
                            text_bytes += app.key.len()
                                + app.name.len()
                                + app.version.as_ref().map_or(0, String::len)
                                + app
                                    .install_path
                                    .as_ref()
                                    .map_or(0, |p| p.as_os_str().len() * 2);
                            if text_bytes > 16 * 1024 * 1024 {
                                inventory.issues.push((
                                    reference.clone(),
                                    "registry text limit reached".into(),
                                ));
                                return Ok(inventory);
                            }
                            // Shared views can expose identical per-user records.
                            // Keep conflicting values and distinct product keys.
                            let identity = (
                                label,
                                name.to_lowercase(),
                                app.name.clone(),
                                app.install_path.clone(),
                                app.version.clone(),
                            );
                            if seen.insert(identity) {
                                inventory.applications.push(app);
                            }
                        }
                        Ok(None) => {}
                        Err(e) => inventory.issues.push((key_reference, e.to_string())),
                    }
                }
            }
        }
        Ok(inventory)
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        struct Fixture(String);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let path = wide(&self.0);
                // SAFETY: removes only this test's uniquely named fixture subtree.
                unsafe {
                    RegDeleteTreeW(HKEY_CURRENT_USER, path.as_ptr());
                }
            }
        }
        #[test]
        fn registry_fixture_reads_unicode_hides_components_bounds_values_and_cannot_write() {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let fixture = Fixture(format!(
                "Software\\DiskCleanupTests\\{}-{unique}",
                std::process::id()
            ));
            let path = wide(&fixture.0);
            let mut handle = ptr::null_mut();
            // SAFETY: terminated fixture name and valid output; writes only owned fixture data.
            status(unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    path.as_ptr(),
                    0,
                    ptr::null(),
                    0,
                    KEY_ALL_ACCESS | KEY_WOW64_64KEY,
                    ptr::null(),
                    &mut handle,
                    ptr::null_mut(),
                )
            })
            .expect("fixture");
            let writable = Key(handle);
            let set = |name: &str, kind: u32, data: &[u8]| {
                let name = wide(name);
                // SAFETY: buffers live for the call and lengths match their allocations.
                status(unsafe {
                    RegSetValueExW(
                        writable.0,
                        name.as_ptr(),
                        0,
                        kind,
                        data.as_ptr(),
                        data.len() as u32,
                    )
                })
                .expect("set");
            };
            let text = |s: &str| {
                wide(s)
                    .iter()
                    .flat_map(|u| u.to_le_bytes())
                    .collect::<Vec<_>>()
            };
            set("DisplayName", REG_SZ, &text("Приложение Ω"));
            set("DisplayVersion", REG_SZ, &text("1.2"));
            set("InstallLocation", REG_SZ, &text(r"D:\Games\Example"));
            let readonly = open(HKEY_CURRENT_USER, &fixture.0, KEY_WOW64_64KEY).expect("read");
            let app = application(&readonly, "fixture".into())
                .expect("row")
                .expect("app");
            assert_eq!(app.name, "Приложение Ω");
            assert_eq!(app.install_path, Some(PathBuf::from(r"D:\Games\Example")));
            // SAFETY: valid handle/name/data; the assertion verifies access rights.
            assert_eq!(
                unsafe {
                    RegSetValueExW(
                        readonly.0,
                        wide("CannotWrite").as_ptr(),
                        0,
                        REG_DWORD,
                        [0u8; 4].as_ptr(),
                        4,
                    )
                },
                windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED
            );
            set("SystemComponent", REG_DWORD, &1u32.to_le_bytes());
            assert!(
                application(&readonly, "fixture".into())
                    .expect("hidden")
                    .is_none()
            );
            set("SystemComponent", REG_DWORD, &0u32.to_le_bytes());
            set("InstallLocation", REG_EXPAND_SZ, &text(r"%TEMP%\App"));
            assert!(
                application(&readonly, "fixture".into())
                    .expect("unknown")
                    .expect("app")
                    .install_path
                    .is_none()
            );
            set("DisplayName", REG_SZ, &[0x00, 0xd8, 0, 0]);
            assert!(application(&readonly, "fixture".into()).is_err());
            set("DisplayName", REG_SZ, &vec![0; MAX_VALUE_BYTES + 2]);
            assert!(application(&readonly, "fixture".into()).is_err());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_paths_accept_only_local_absolute_directories() {
        for s in [r"C:\Games\One", r#""D:\Program Files\App""#, "E:/Games/App"] {
            assert!(installation_path(s).is_some(), "{s}");
        }
        for s in [
            "",
            "C:",
            r"C:\",
            r"C:relative",
            r"\rooted",
            r"\\server\share\App",
            r"\\?\C:\App",
            r"C:\a\..\b",
            r"C:\a\.\b",
            r"%LOCALAPPDATA%\App",
            r"C:\App:stream",
            "C:/App\0x",
        ] {
            assert!(installation_path(s).is_none(), "{s}");
        }
    }
    #[test]
    fn registry_cancel_precedes_platform_access() {
        assert_eq!(
            discover(|| true).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        #[cfg(not(windows))]
        assert_eq!(
            discover(|| false).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }
}
