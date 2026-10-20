use super::*;
use std::{
    ffi::OsStr,
    os::windows::ffi::OsStrExt,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{DataExchange::*, Memory::*},
    UI::{Shell::ShellExecuteW, WindowsAndMessaging::*},
};
fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}
struct Clipboard;
impl Clipboard {
    fn open(owner: HWND) -> Result<Self> {
        for _ in 0..5 {
            if unsafe { OpenClipboard(owner) } != 0 {
                return Ok(Self);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err(CoreError::new(
            "clipboard_busy",
            "剪贴板正被其他应用使用，请重试",
        ))
    }
}
impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}
pub fn read_clipboard() -> Result<String> {
    let _clipboard = Clipboard::open(null_mut())?;
    let handle = unsafe { GetClipboardData(13) }; // CF_UNICODETEXT
    if handle.is_null() {
        return Ok(String::new());
    }
    let size = unsafe { GlobalSize(handle) };
    if size > 2 * 1024 * 1024 {
        return Err(CoreError::new("clipboard_limit", "剪贴板文本过大，未读取"));
    }
    let ptr = unsafe { GlobalLock(handle) } as *const u16;
    if ptr.is_null() {
        return Err(CoreError::io(
            "clipboard_lock",
            std::io::Error::last_os_error(),
        ));
    }
    let units = unsafe { std::slice::from_raw_parts(ptr, size / 2) };
    let end = units.iter().position(|u| *u == 0).unwrap_or(units.len());
    let text = String::from_utf16_lossy(&units[..end]);
    unsafe {
        GlobalUnlock(handle);
    }
    Ok(text)
}
pub fn write_clipboard(text: &str, owner: isize) -> Result<()> {
    if text.len() > 4 * 1024 * 1024 || text.contains('\0') {
        return Err(CoreError::new("clipboard_limit", "复制内容过大或包含 NUL"));
    }
    let units = wide(text);
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, units.len() * 2) };
    if handle.is_null() {
        return Err(CoreError::io(
            "clipboard_alloc",
            std::io::Error::last_os_error(),
        ));
    }
    let ptr = unsafe { GlobalLock(handle) } as *mut u16;
    if ptr.is_null() {
        unsafe {
            GlobalFree(handle);
        }
        return Err(CoreError::io(
            "clipboard_lock",
            std::io::Error::last_os_error(),
        ));
    }
    unsafe {
        std::ptr::copy_nonoverlapping(units.as_ptr(), ptr, units.len());
        GlobalUnlock(handle);
    }
    let result = (|| {
        let _clipboard = Clipboard::open(owner as HWND)?;
        if unsafe { EmptyClipboard() } == 0 || unsafe { SetClipboardData(13, handle) }.is_null() {
            return Err(CoreError::io(
                "clipboard_write",
                std::io::Error::last_os_error(),
            ));
        }
        Ok(())
    })();
    if result.is_err() {
        unsafe {
            GlobalFree(handle);
        }
    } // On success the OS owns the allocation.
    result
}
pub fn open_link(value: &str) -> Result<()> {
    let url = validate_link(value)?;
    let uri = wide(url.as_str());
    let open = wide("open");
    if unsafe {
        ShellExecuteW(
            null_mut(),
            open.as_ptr(),
            uri.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    } as isize
        <= 32
    {
        return Err(CoreError::new("open_link", "无法打开默认浏览器"));
    }
    Ok(())
}
pub fn confirm(message: &str) -> bool {
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(message).as_ptr(),
            wide("Terminder").as_ptr(),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
        ) == IDYES
    }
}
pub fn show_error(message: &str) {
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(message).as_ptr(),
            wide("Terminder").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
