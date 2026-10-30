use crate::model::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub platforms: Vec<String>,
    pub resolver: Option<String>,
    pub program: Option<PathBuf>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_cwd_policy")]
    pub cwd_policy: String,
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: BTreeMap<String, Option<String>>,
}
fn default_cwd_policy() -> String {
    "inherit_known".into()
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty()
            || self.id == "auto"
            || self.id.len() > 64
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || self.name.is_empty()
            || self.name.chars().count() > 80
        {
            return Err(CoreError::new("invalid_profile", "Profile 标识或名称无效"));
        }
        if self.resolver.is_some() == self.program.is_some() {
            return Err(CoreError::new(
                "invalid_profile",
                "resolver 和 program 必须且只能指定一个",
            ));
        }
        if self
            .resolver
            .as_deref()
            .is_some_and(|r| !["pwsh", "windows-powershell", "cmd"].contains(&r))
        {
            return Err(CoreError::new("unknown_resolver", "未知 Shell 发现规则"));
        }
        if self.platforms.is_empty()
            || self
                .platforms
                .iter()
                .any(|p| !["windows", "macos", "linux"].contains(&p.as_str()))
        {
            return Err(CoreError::new("invalid_platform", "Profile 平台无效"));
        }
        if self.program.as_ref().is_some_and(|p| !p.is_absolute()) {
            return Err(CoreError::new(
                "invalid_program",
                "自定义程序必须使用绝对路径",
            ));
        }
        if !["inherit_known", "home", "fixed"].contains(&self.cwd_policy.as_str())
            || (self.cwd_policy == "fixed" && self.cwd.is_none())
        {
            return Err(CoreError::new(
                "invalid_cwd_policy",
                "目录策略无效或缺少预设目录",
            ));
        }
        if self.cwd.as_ref().is_some_and(|p| !p.is_absolute()) {
            return Err(CoreError::new("invalid_cwd", "预设目录必须使用绝对路径"));
        }
        if self.args.len() > 128 || self.args.iter().any(|s| s.len() > 8192 || s.contains('\0')) {
            return Err(CoreError::new("invalid_args", "参数过长或包含 NUL"));
        }
        let mut keys = BTreeSet::new();
        for (key, value) in &self.env {
            #[cfg(windows)]
            let canonical = crate::windows_env::EnvironmentKey::new(key.as_ref());
            #[cfg(not(windows))]
            let canonical = key.clone();
            if key.is_empty()
                || key.contains(['=', '\0'])
                || key.len() > 1024
                || value
                    .as_ref()
                    .is_some_and(|v| v.contains('\0') || v.len() > 32768)
                || !keys.insert(canonical)
            {
                return Err(CoreError::new(
                    "invalid_env",
                    "环境变量名称、长度或大小写重复无效",
                ));
            }
        }
        Ok(())
    }
}

pub fn builtin_profiles() -> Vec<Profile> {
    [
        ("pwsh", "PowerShell 7", vec!["-NoLogo"]),
        ("windows-powershell", "PowerShell", vec!["-NoLogo"]),
        ("cmd", "Command Prompt", vec![]),
    ]
    .into_iter()
    .map(|(id, name, args)| Profile {
        id: id.into(),
        name: name.into(),
        platforms: vec!["windows".into()],
        resolver: Some(id.into()),
        program: None,
        args: args.into_iter().map(str::to_owned).collect(),
        cwd_policy: default_cwd_policy(),
        cwd: None,
        env: BTreeMap::new(),
    })
    .collect()
}

pub fn home_dir() -> Result<PathBuf> {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    env::var_os(key)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_dir())
        .ok_or_else(|| CoreError::new("home_missing", "无法定位用户 Home 目录"))
}

pub fn resolve_program(profile: &Profile) -> Result<PathBuf> {
    profile.validate()?;
    if !profile.platforms.iter().any(|p| p == env::consts::OS) {
        return Err(CoreError::new(
            "unsupported_platform",
            "此 Profile 不适用于当前平台",
        ));
    }
    if let Some(program) = &profile.program {
        return match std::fs::metadata(program) {
            Ok(metadata) if metadata.is_file() => Ok(program.clone()),
            Ok(_) => Err(CoreError::new("program_missing", "指定程序不是文件")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(CoreError::new("program_missing", "指定程序不存在"))
            }
            Err(error) => Err(CoreError::io("discover_shell", error)),
        };
    }
    let resolver = profile.resolver.as_deref().unwrap_or_default();
    let mut candidates = Vec::new();
    if resolver == "pwsh" {
        if let Some(root) = env::var_os("ProgramFiles") {
            candidates.push(PathBuf::from(root).join("PowerShell/7/pwsh.exe"));
        }
        if let Some(path) = env::var_os("PATH") {
            candidates.extend(
                env::split_paths(&path)
                    .filter(|p| p.is_absolute())
                    .map(|p| p.join("pwsh.exe")),
            );
        }
    } else if let Some(root) = env::var_os("SystemRoot") {
        let system = PathBuf::from(root).join("System32");
        candidates.push(if resolver == "cmd" {
            system.join("cmd.exe")
        } else {
            system.join("WindowsPowerShell/v1.0/powershell.exe")
        });
    }
    for path in candidates {
        match std::fs::metadata(&path) {
            Ok(meta) if meta.is_file() => return Ok(path),
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(CoreError::io("discover_shell", error)),
        }
    }
    Err(CoreError::new(
        "program_missing",
        "未发现该 Shell，请安装或配置绝对程序路径",
    ))
}

pub fn resolve_profile(profiles: &[Profile], requested: &str) -> Result<(Profile, PathBuf)> {
    if requested != "auto" {
        let profile = profiles
            .iter()
            .find(|p| p.id == requested)
            .ok_or_else(|| CoreError::new("profile_missing", "Profile 已不存在"))?;
        return Ok((profile.clone(), resolve_program(profile)?));
    }
    for id in ["pwsh", "windows-powershell", "cmd"] {
        if let Some(profile) = profiles.iter().find(|p| p.id == id) {
            match resolve_program(profile) {
                Ok(path) => return Ok((profile.clone(), path)),
                Err(error)
                    if ["program_missing", "unsupported_platform"]
                        .contains(&error.code.as_str()) =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
    }
    Err(CoreError::new(
        "no_shell",
        "没有可用的内置 Shell，请在设置中添加 Profile",
    ))
}

pub fn spawn_spec(
    profile: Profile,
    program: PathBuf,
    explicit_cwd: Option<&Path>,
    inherited_cwd: Option<&Path>,
) -> Result<SpawnSpec> {
    let (cwd, source) = if let Some(path) = explicit_cwd {
        if !path.is_absolute() || !path.is_dir() {
            return Err(CoreError::new(
                "invalid_cwd",
                "指定启动目录无效，请修正后重试",
            ));
        }
        (path.to_path_buf(), "explicit")
    } else if profile.cwd_policy == "inherit_known"
        && inherited_cwd.is_some_and(|p| p.is_absolute() && p.is_dir())
    {
        (
            inherited_cwd.expect("checked").to_path_buf(),
            "inherited_startup",
        )
    } else if profile.cwd_policy != "home" && profile.cwd.as_ref().is_some_and(|p| p.is_dir()) {
        (profile.cwd.clone().expect("checked"), "profile")
    } else if profile.cwd_policy == "fixed" {
        return Err(CoreError::new("invalid_cwd", "Profile 的固定目录不存在"));
    } else {
        (home_dir()?, "home")
    };
    let mut vars: BTreeMap<OsString, Option<OsString>> = BTreeMap::from([
        ("TERM_PROGRAM".into(), Some("Terminder".into())),
        (
            "TERM_PROGRAM_VERSION".into(),
            Some(env!("CARGO_PKG_VERSION").into()),
        ),
        ("COLORTERM".into(), Some("truecolor".into())),
    ]);
    vars.extend(
        profile
            .env
            .into_iter()
            .map(|(k, v)| (k.into(), v.map(Into::into))),
    );
    let spec = SpawnSpec {
        profile_id: profile.id,
        profile_name: profile.name,
        program,
        args: profile.args.into_iter().map(Into::into).collect(),
        env: vars,
        cwd,
        cwd_source: source.into(),
        size: TerminalSize::default(),
    };
    spec.validate()?;
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn program_and_resolver_are_exclusive() {
        let mut p = builtin_profiles().remove(0);
        p.program = Some(PathBuf::from("relative.exe"));
        assert!(p.validate().is_err());
    }
    #[test]
    fn defaults_respect_user_profiles() {
        assert!(builtin_profiles().iter().all(|p| {
            !p.args
                .iter()
                .any(|a| a == "-NoProfile" || a == "-ExecutionPolicy")
        }));
    }
    #[test]
    fn explicit_bad_directory_is_never_replaced_by_home() {
        let p = builtin_profiles().remove(0);
        assert_eq!(
            spawn_spec(
                p,
                std::env::current_exe().unwrap(),
                Some(Path::new("relative")),
                None
            )
            .unwrap_err()
            .code,
            "invalid_cwd"
        );
    }
}
