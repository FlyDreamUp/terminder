//! The app-local Microsoft ConPTY pair avoids the system implementation's
//! leaked conhost process handle on Windows 10 build 19044.
use super::*;
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
type Create = unsafe extern "system" fn(COORD, HANDLE, HANDLE, u32, *mut HPCON) -> i32;
type Resize = unsafe extern "system" fn(HPCON, COORD) -> i32;
type Close = unsafe extern "system" fn(HPCON);
pub(super) struct Conpty {
    module: HMODULE,
    create: Create,
    resize: Resize,
    close: Close,
}
// SAFETY: the module stays loaded for every owning Native and close worker.
// Function pointers are immutable; each HPCON still has one exclusive owner.
unsafe impl Send for Conpty {}
unsafe impl Sync for Conpty {}
impl Conpty {
    pub(super) fn load() -> Result<Arc<Self>> {
        let executable = std::env::current_exe().map_err(|e| CoreError::io("conpty_path", e))?;
        let directory = executable
            .parent()
            .ok_or_else(|| CoreError::new("conpty_path", "无法定位 ConPTY 组件目录"))?;
        for name in ["conpty.dll", "OpenConsole.exe"] {
            if !directory.join(name).is_file() {
                return Err(CoreError::new(
                    "conpty_missing",
                    format!("缺少 ConPTY 组件 {name}，请完整解压应用安装包"),
                ));
            }
        }
        let file = wide(directory.join("conpty.dll").as_os_str())?;
        // Only the executable's directory and Windows system directory are searched.
        // Never load a same-named DLL from the shell's cwd or PATH.
        let module = unsafe {
            LoadLibraryExW(
                file.as_ptr(),
                null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        if module.is_null() {
            return Err(last("conpty_load"));
        }
        let loaded = (|| {
            let symbol = |name: &std::ffi::CStr| {
                unsafe { GetProcAddress(module, name.as_ptr().cast()) }
                    .ok_or_else(|| last("conpty_export"))
            };
            // SAFETY: signatures match the pinned package's inc/conpty.h.
            unsafe {
                Ok(Self {
                    module,
                    create: std::mem::transmute::<unsafe extern "system" fn() -> isize, Create>(
                        symbol(c"ConptyCreatePseudoConsole")?,
                    ),
                    resize: std::mem::transmute::<unsafe extern "system" fn() -> isize, Resize>(
                        symbol(c"ConptyResizePseudoConsole")?,
                    ),
                    close: std::mem::transmute::<unsafe extern "system" fn() -> isize, Close>(
                        symbol(c"ConptyClosePseudoConsole")?,
                    ),
                })
            }
        })();
        if loaded.is_err() {
            unsafe {
                FreeLibrary(module);
            }
        }
        loaded.map(Arc::new)
    }
    pub(super) unsafe fn create(
        &self,
        size: COORD,
        input: HANDLE,
        output: HANDLE,
        console: *mut HPCON,
    ) -> i32 {
        unsafe { (self.create)(size, input, output, 0, console) }
    }
    pub(super) unsafe fn resize(&self, console: HPCON, size: COORD) -> i32 {
        unsafe { (self.resize)(console, size) }
    }
    pub(super) unsafe fn close(&self, console: HPCON) {
        unsafe { (self.close)(console) }
    }
}
impl Drop for Conpty {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.module);
        }
    }
}
