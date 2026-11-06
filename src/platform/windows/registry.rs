//! Thin helpers over the per-user registry (`HKEY_CURRENT_USER`), shared by the login item and the
//! installer. Every function takes the key path below `HKCU` and reports failures as
//! [`std::io::Error`].

use super::util::{from_wide, utf16_bytes_to_string, wide};
use std::io;
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE,
    KEY_SET_VALUE, KEY_WOW64_32KEY, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteKeyW, RegDeleteValueW, RegEnumKeyExW, RegOpenKeyExW,
    RegQueryValueExW, RegSetValueExW,
};

/// Turns a Win32 status code into a `Result`.
fn check(code: u32) -> io::Result<()> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code as i32))
    }
}

/// An open key that is closed again when dropped.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful open or create call and is closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

/// Opens an existing key. `Ok(None)` when it does not exist.
fn open(path: &str, access: u32) -> io::Result<Option<Key>> {
    let mut key: HKEY = std::ptr::null_mut();
    let path = wide(path);
    // SAFETY: `key` is a valid out-pointer and `path` is zero terminated.
    let code = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut key) };
    match code {
        ERROR_SUCCESS => Ok(Some(Key(key))),
        ERROR_FILE_NOT_FOUND => Ok(None),
        other => check(other).map(|()| None),
    }
}

/// Opens a key, creating it (and its parents) when it is missing.
fn create(path: &str) -> io::Result<Key> {
    let mut key: HKEY = std::ptr::null_mut();
    let path = wide(path);
    // SAFETY: the out-pointers are valid, `path` is zero terminated, and null is allowed for the
    // class, the security attributes and the disposition.
    let code = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            path.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        )
    };
    check(code).map(|()| Key(key))
}

/// Writes a string value, creating the key when needed.
pub fn set_string(path: &str, name: &str, value: &str) -> io::Result<()> {
    let key = create(path)?;
    let (name, value) = (wide(name), wide(value));
    // SAFETY: the data pointer and byte length describe `value`, which outlives the call.
    let code = unsafe {
        RegSetValueExW(
            key.0,
            name.as_ptr(),
            0,
            REG_SZ,
            value.as_ptr().cast(),
            (value.len() * 2) as u32,
        )
    };
    check(code)
}

/// Writes a `DWORD` value, creating the key when needed.
pub fn set_dword(path: &str, name: &str, value: u32) -> io::Result<()> {
    let key = create(path)?;
    let name = wide(name);
    // SAFETY: the data pointer and byte length describe the local `value`.
    let code = unsafe {
        RegSetValueExW(
            key.0,
            name.as_ptr(),
            0,
            REG_DWORD,
            (&value as *const u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
    check(code)
}

/// Writes a binary value (the `StartupApproved` flags), creating the key when needed.
pub fn set_binary(path: &str, name: &str, data: &[u8]) -> io::Result<()> {
    use windows_sys::Win32::System::Registry::REG_BINARY;
    let key = create(path)?;
    let name = wide(name);
    // SAFETY: the pointer and length describe `data`, which outlives the call.
    let code = unsafe {
        RegSetValueExW(
            key.0,
            name.as_ptr(),
            0,
            REG_BINARY,
            data.as_ptr(),
            data.len() as u32,
        )
    };
    check(code)
}

/// The raw bytes of a value of an open key, or `None` when the value does not exist. The empty name
/// is the key's default value.
fn read_value(key: &Key, name: &str) -> Option<Vec<u8>> {
    let name = wide(name);
    let mut len: u32 = 0;
    // SAFETY: with a null data pointer the call only reports the size.
    let code = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut len,
        )
    };
    if code != ERROR_SUCCESS {
        return None;
    }
    let mut buf = vec![0u8; len as usize];
    // SAFETY: `buf` holds `len` bytes, which is what the call may write.
    let code = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            buf.as_mut_ptr(),
            &mut len,
        )
    };
    (code == ERROR_SUCCESS).then(|| {
        buf.truncate(len as usize);
        buf
    })
}

/// The raw bytes of a value, or `None` when the key or the value does not exist.
pub fn get_bytes(path: &str, name: &str) -> Option<Vec<u8>> {
    let key = open(path, KEY_QUERY_VALUE).ok()??;
    read_value(&key, name)
}

/// The registry hives that are read (never written) by the helpers below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

/// Opens an existing key of `hive` for reading. `None` when it does not exist or cannot be read.
fn open_hive(hive: Hive, path: &str, access: u32) -> Option<Key> {
    let root = match hive {
        Hive::CurrentUser => HKEY_CURRENT_USER,
        Hive::LocalMachine => HKEY_LOCAL_MACHINE,
    };
    let mut key: HKEY = std::ptr::null_mut();
    let path = wide(path);
    // SAFETY: `key` is a valid out-pointer and `path` is zero terminated.
    let code = unsafe { RegOpenKeyExW(root, path.as_ptr(), 0, access, &mut key) };
    (code == ERROR_SUCCESS).then(|| Key(key))
}

/// `wow32` looks at the 32-bit view of the registry (`WOW6432Node`) on a 64-bit Windows.
fn view(wow32: bool) -> u32 {
    if wow32 { KEY_WOW64_32KEY } else { 0 }
}

/// The names of the subkeys of a key. Empty when the key does not exist.
pub fn subkeys(hive: Hive, path: &str, wow32: bool) -> Vec<String> {
    let Some(key) = open_hive(hive, path, KEY_ENUMERATE_SUB_KEYS | view(wow32)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for index in 0u32..100_000 {
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        // SAFETY: `buf` and `len` describe the same writable buffer; the optional parameters are null.
        let code = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                buf.as_mut_ptr(),
                &mut len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        match code {
            ERROR_SUCCESS => out.push(from_wide(&buf[..len as usize])),
            ERROR_MORE_DATA => {} // a name longer than 511 characters is not an app: skip it
            _ => break,           // ERROR_NO_MORE_ITEMS or a real failure
        }
    }
    out
}

/// The default value of a key as text (`REG_SZ` and `REG_EXPAND_SZ`, not expanded).
pub fn default_string(hive: Hive, path: &str, wow32: bool) -> Option<String> {
    let key = open_hive(hive, path, KEY_QUERY_VALUE | view(wow32))?;
    let text = utf16_bytes_to_string(&read_value(&key, "")?);
    (!text.trim().is_empty()).then_some(text)
}

/// Whether the value exists.
pub fn has_value(path: &str, name: &str) -> bool {
    get_bytes(path, name).is_some()
}

/// Deletes a value. A value or key that is already gone counts as success.
pub fn delete_value(path: &str, name: &str) -> io::Result<()> {
    let Some(key) = open(path, KEY_SET_VALUE)? else {
        return Ok(());
    };
    let name = wide(name);
    // SAFETY: `name` is zero terminated.
    match unsafe { RegDeleteValueW(key.0, name.as_ptr()) } {
        ERROR_FILE_NOT_FOUND => Ok(()),
        other => check(other),
    }
}

/// Deletes a key without subkeys. A key that is already gone counts as success.
pub fn delete_key(path: &str) -> io::Result<()> {
    let path = wide(path);
    // SAFETY: `path` is zero terminated.
    match unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, path.as_ptr()) } {
        ERROR_FILE_NOT_FOUND => Ok(()),
        other => check(other),
    }
}
