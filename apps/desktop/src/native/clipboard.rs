//! Windows text copying with explicit ownership, bounded contention retries and errors.
use anyhow::{Context as _, Result};
use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{io, thread, time::Duration};
use windows_sys::Win32::{
    Foundation::{GlobalFree, HGLOBAL, HWND},
    System::{
        DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        Ole::CF_UNICODETEXT,
    },
};

struct OpenClipboardGuard;
impl OpenClipboardGuard {
    fn open(owner: HWND) -> Result<Self> {
        // Clipboard history/readers can briefly hold the shared clipboard after a copy.
        // Only retry acquiring it, never repeat EmptyClipboard/SetClipboardData.
        for attempt in 0..5 {
            if unsafe { OpenClipboard(owner) } != 0 {
                return Ok(Self);
            }
            let error = io::Error::last_os_error();
            if attempt == 4 {
                return Err(error).context("无法打开剪贴板，请稍后重试");
            }
            thread::sleep(Duration::from_millis(5));
        }
        unreachable!()
    }
}
impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        if unsafe { CloseClipboard() } == 0 {
            tracing::warn!(error = %io::Error::last_os_error(), "CloseClipboard failed");
        }
    }
}
struct GlobalText(HGLOBAL);
impl GlobalText {
    fn new(text: &str) -> Result<Self> {
        anyhow::ensure!(!text.contains('\0'), "复制内容包含空字符");
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let allocation =
            Self(unsafe { GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of_val(wide.as_slice())) });
        anyhow::ensure!(
            !allocation.0.is_null(),
            "GlobalAlloc: {}",
            io::Error::last_os_error()
        );
        let buffer = unsafe { GlobalLock(allocation.0) }.cast::<u16>();
        anyhow::ensure!(
            !buffer.is_null(),
            "GlobalLock: {}",
            io::Error::last_os_error()
        );
        // The movable allocation has exactly enough space for the UTF-16 text and NUL.
        unsafe {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), buffer, wide.len());
            // A zero return is normal when the final lock is released.
            GlobalUnlock(allocation.0);
        }
        Ok(allocation)
    }
}
impl Drop for GlobalText {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                GlobalFree(self.0);
            }
        }
    }
}

pub(super) fn write_text(window: &Window, text: &str) -> Result<()> {
    let RawWindowHandle::Win32(handle) = HasWindowHandle::window_handle(window)
        .map_err(|error| anyhow::anyhow!("window handle: {error}"))?
        .as_raw()
    else {
        anyhow::bail!("复制需要有效的 Windows 窗口");
    };
    // Allocate before changing the clipboard, so allocation failure preserves its contents.
    let allocation = GlobalText::new(text)?;
    let _clipboard = OpenClipboardGuard::open(handle.hwnd.get() as HWND)?;
    if unsafe { EmptyClipboard() } == 0 {
        return Err(io::Error::last_os_error()).context("EmptyClipboard");
    }
    if unsafe { SetClipboardData(u32::from(CF_UNICODETEXT), allocation.0) }.is_null() {
        return Err(io::Error::last_os_error()).context("SetClipboardData");
    }
    // Windows owns the movable allocation only after SetClipboardData succeeds.
    std::mem::forget(allocation);
    Ok(())
}
