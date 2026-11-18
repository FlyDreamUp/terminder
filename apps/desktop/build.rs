use std::{env, fs, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("output dir"));
    let icon = root
        .join("assets/icon.ico")
        .display()
        .to_string()
        .replace('\\', "\\\\");
    let version = env::var("CARGO_PKG_VERSION").expect("package version");
    let tuple = version.split('.').take(3).collect::<Vec<_>>().join(",");
    let source = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {tuple},0
PRODUCTVERSION {tuple},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
 BLOCK "StringFileInfo"
 BEGIN
  BLOCK "040904B0"
  BEGIN
   VALUE "CompanyName", "Terminder"
   VALUE "FileDescription", "Terminder native terminal"
   VALUE "FileVersion", "{version}"
   VALUE "ProductName", "Terminder"
   VALUE "ProductVersion", "{version}"
   VALUE "OriginalFilename", "terminder-desktop.exe"
  END
 END
 BLOCK "VarFileInfo"
 BEGIN
  VALUE "Translation", 0x0409, 1200
 END
END
"#
    );
    let rc = out.join("terminder.rc");
    let resource = out.join("terminder.res");
    fs::write(&rc, source).expect("write Windows resources");
    let compiler = find_rc().expect("Windows SDK rc.exe is required to build Terminder");
    let status = Command::new(compiler)
        .args(["/nologo", "/c65001", "/fo"])
        .arg(&resource)
        .arg(&rc)
        .status()
        .expect("run Windows resource compiler");
    assert!(status.success(), "Windows resource compilation failed");
    println!(
        "cargo:rustc-link-arg-bin=terminder-desktop={}",
        resource.display()
    );
}
fn find_rc() -> Option<PathBuf> {
    if let Some(path) = env::var_os("PATH") {
        for dir in env::split_paths(&path) {
            let file = dir.join("rc.exe");
            if file.is_file() {
                return Some(file);
            }
        }
    }
    let kits = env::var_os("WindowsSdkDir")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("ProgramFiles(x86)").map(|p| PathBuf::from(p).join("Windows Kits/10"))
        })?;
    let mut versions = fs::read_dir(kits.join("bin"))
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    versions.sort();
    versions
        .into_iter()
        .rev()
        .map(|p| p.join("x64/rc.exe"))
        .find(|p| p.is_file())
}
