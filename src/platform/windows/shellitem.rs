//! Two shell COM objects, called through their vtables: the icon of a file or app
//! (`IShellItemImageFactory`) and the target of a `.lnk` shortcut (`IShellLinkW`).
//!
//! `windows-sys` has the functions but not the COM interfaces, and the project does not want a
//! second Windows crate for two calls each, so the few vtable slots that are needed are declared
//! here. The order of the slots is the order of the interface in the Windows headers; a slot that
//! is not called is declared as a plain `usize` placeholder.
//!
//! Every function needs COM on the calling thread: call [`super::apps::com_for_thread`] first.

use super::apps::bitmap_bgra;
use super::util::{from_wide, shell_bgra_to_rgba, wide};
use std::ffi::c_void;
use windows_sys::Win32::Foundation::SIZE;
use windows_sys::Win32::Graphics::Gdi::{BITMAP, DeleteObject, GetObjectW, HBITMAP};
use windows_sys::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows_sys::Win32::UI::Shell::SHCreateItemFromParsingName;
use windows_sys::core::GUID;

/// `SIIGBF_BIGGERSIZEOK`: a bigger icon than asked for is fine (it is scaled down by the caller).
const SIIGBF_BIGGERSIZEOK: u32 = 0x0000_0001;
/// `SIIGBF_ICONONLY`: the icon of the file, never a thumbnail of its content.
const SIIGBF_ICONONLY: u32 = 0x0000_0004;
/// `STGM_READ`.
const STGM_READ: u32 = 0;

/// `IID_IShellItemImageFactory`
static IID_IMAGE_FACTORY: GUID = GUID {
    data1: 0xBCC1_8B79,
    data2: 0xBA16,
    data3: 0x442F,
    data4: [0x80, 0xC4, 0x8A, 0x59, 0xC3, 0x0C, 0x46, 0x3B],
};
/// `CLSID_ShellLink`
static CLSID_SHELL_LINK: GUID = GUID {
    data1: 0x0002_1401,
    data2: 0,
    data3: 0,
    data4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
};
/// `IID_IShellLinkW`
static IID_SHELL_LINK_W: GUID = GUID {
    data1: 0x0002_14F9,
    data2: 0,
    data3: 0,
    data4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
};
/// `IID_IPersistFile`
static IID_PERSIST_FILE: GUID = GUID {
    data1: 0x0000_010B,
    data2: 0,
    data3: 0,
    data4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
};

/// The three slots every COM interface starts with.
#[repr(C)]
struct UnknownVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    _add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

/// `IShellItemImageFactory`: `IUnknown` and `GetImage(SIZE, SIIGBF, HBITMAP*)`.
#[repr(C)]
struct ImageFactoryVtbl {
    _unknown: UnknownVtbl,
    get_image: unsafe extern "system" fn(*mut c_void, SIZE, u32, *mut HBITMAP) -> i32,
}

/// `IShellLinkW`: `IUnknown` and `GetPath(buffer, length, WIN32_FIND_DATAW*, flags)`.
#[repr(C)]
struct ShellLinkVtbl {
    _unknown: UnknownVtbl,
    get_path: unsafe extern "system" fn(*mut c_void, *mut u16, i32, *mut c_void, u32) -> i32,
}

/// `IPersistFile`: `IUnknown`, `GetClassID`, `IsDirty` and `Load(path, mode)`.
#[repr(C)]
struct PersistFileVtbl {
    _unknown: UnknownVtbl,
    _get_class_id: usize,
    _is_dirty: usize,
    load: unsafe extern "system" fn(*mut c_void, *const u16, u32) -> i32,
}

/// An owned COM interface pointer, released when dropped.
struct Com(*mut c_void);

impl Com {
    /// The vtable of the object, as the interface `T` that this pointer really is.
    fn vtbl<T>(&self) -> &T {
        // SAFETY: a COM interface pointer points at a pointer to its vtable, which lives as long
        // as the object, and the callers name the interface this pointer was created as.
        unsafe { &**(self.0 as *const *const T) }
    }

    /// Asks the object for another of its interfaces.
    fn query(&self, iid: &GUID) -> Option<Com> {
        let mut out: *mut c_void = std::ptr::null_mut();
        // SAFETY: `iid` and `out` are valid for the call; `self` is a live COM object.
        let hr = unsafe { (self.vtbl::<UnknownVtbl>().query_interface)(self.0, iid, &mut out) };
        (hr >= 0 && !out.is_null()).then(|| Com(out))
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        // SAFETY: balances the reference this value owns; the pointer is not used afterwards.
        unsafe { (self.vtbl::<UnknownVtbl>().release)(self.0) };
    }
}

/// The icon of a file or app as straight RGBA, at most `px` x `px` (the shell may hand out a
/// bigger one). `name` is what the shell can parse: a file path, or `shell:AppsFolder\<AUMID>`.
/// This is the same icon Explorer shows, with its alpha channel, for executables, shortcuts and
/// Store apps alike.
pub fn shell_icon_rgba(name: &str, px: i32) -> Option<(u32, u32, Vec<u8>)> {
    let name = wide(name);
    let mut raw: *mut c_void = std::ptr::null_mut();
    // SAFETY: `name` is zero terminated; a null bind context is allowed; `raw` is a valid out-pointer.
    let hr = unsafe {
        SHCreateItemFromParsingName(
            name.as_ptr(),
            std::ptr::null_mut(),
            &IID_IMAGE_FACTORY,
            &mut raw,
        )
    };
    if hr < 0 || raw.is_null() {
        return None;
    }
    let item = Com(raw);
    let mut bitmap: HBITMAP = std::ptr::null_mut();
    // SAFETY: `item` is a live IShellItemImageFactory and `bitmap` a valid out-pointer.
    let hr = unsafe {
        (item.vtbl::<ImageFactoryVtbl>().get_image)(
            item.0,
            SIZE { cx: px, cy: px },
            SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
            &mut bitmap,
        )
    };
    if hr < 0 || bitmap.is_null() {
        return None;
    }
    let icon = read_bitmap(bitmap);
    // SAFETY: the bitmap is ours (the shell gave away its ownership) and is not used afterwards.
    unsafe { DeleteObject(bitmap) };
    icon
}

/// The pixels of a 32-bit shell bitmap as straight RGBA.
fn read_bitmap(bitmap: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    // SAFETY: `bm` is a valid out-buffer of the size that is passed along.
    let (w, h) = unsafe {
        let mut bm: BITMAP = std::mem::zeroed();
        let got = GetObjectW(
            bitmap,
            std::mem::size_of::<BITMAP>() as i32,
            (&mut bm as *mut BITMAP).cast(),
        );
        if got == 0 {
            return None;
        }
        (bm.bmWidth, bm.bmHeight)
    };
    if !(1..=4096).contains(&w) || !(1..=4096).contains(&h) {
        return None;
    }
    // SAFETY: the bitmap handle is valid and `w` x `h` are its real dimensions.
    let mut px = unsafe { bitmap_bgra(bitmap, w, h) }?;
    shell_bgra_to_rgba(&mut px);
    Some((w as u32, h as u32, px))
}

/// The program a `.lnk` shortcut starts, or `None` when it cannot be read or has no file target
/// (a shortcut to a folder, a Store app or a web address).
pub fn shortcut_target(lnk: &str) -> Option<String> {
    let mut raw: *mut c_void = std::ptr::null_mut();
    // SAFETY: the GUIDs are valid, `raw` is a valid out-pointer; a null outer object is allowed.
    let hr = unsafe {
        CoCreateInstance(
            &CLSID_SHELL_LINK,
            std::ptr::null_mut(),
            CLSCTX_INPROC_SERVER as _,
            &IID_SHELL_LINK_W,
            &mut raw,
        )
    };
    if hr < 0 || raw.is_null() {
        return None;
    }
    let link = Com(raw);
    let file = link.query(&IID_PERSIST_FILE)?;
    let path = wide(lnk);
    // SAFETY: `file` is a live IPersistFile and `path` is zero terminated.
    if unsafe { (file.vtbl::<PersistFileVtbl>().load)(file.0, path.as_ptr(), STGM_READ) } < 0 {
        return None;
    }
    let mut buf = [0u16; 1024];
    // SAFETY: `buf` and its length describe the same writable buffer; the find data is optional.
    let hr = unsafe {
        (link.vtbl::<ShellLinkVtbl>().get_path)(
            link.0,
            buf.as_mut_ptr(),
            buf.len() as i32,
            std::ptr::null_mut(),
            0,
        )
    };
    if hr != 0 {
        return None; // S_FALSE: the shortcut has no path
    }
    let target = from_wide(&buf);
    (!target.is_empty()).then_some(target)
}
