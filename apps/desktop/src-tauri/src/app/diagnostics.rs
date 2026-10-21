//! Diagnostic-build-only WebView2 options. Ordinary builds do not compile this module.
use std::{ffi::OsStr, num::NonZeroU16};
use terminder_session_core::model::{CoreError, Result};

pub fn browser_arguments(port: Option<&OsStr>, isolated_data: bool) -> Result<Option<String>> {
    let Some(port) = port else {
        return Ok(None);
    };
    if !isolated_data {
        return Err(CoreError::new(
            "test_cdp_root",
            "Diagnostic CDP requires TERMINDER_TEST_DATA_ROOT",
        ));
    }
    let port = port
        .to_str()
        .and_then(|value| value.parse::<NonZeroU16>().ok())
        .ok_or_else(|| {
            CoreError::new("test_cdp_port", "Diagnostic CDP port must be in 1..=65535")
        })?;
    // Elevated WebView2 ignores its environment overrides. Set API options instead,
    // preserving Wry 0.57's default disabled features and autoplay policy.
    Ok(Some(format!(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
         --autoplay-policy=no-user-gesture-required \
         --remote-debugging-address=127.0.0.1 --remote-debugging-port={port}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debugging_is_disabled_without_an_explicit_port() {
        assert_eq!(browser_arguments(None, false).unwrap(), None);
        assert_eq!(browser_arguments(None, true).unwrap(), None);
    }

    #[test]
    fn debugging_requires_isolated_application_data() {
        let error = browser_arguments(Some(OsStr::new("9427")), false).unwrap_err();
        assert_eq!(error.code, "test_cdp_root");
    }

    #[test]
    fn invalid_ports_and_browser_flag_injection_are_rejected() {
        for port in [
            "",
            "0",
            "65536",
            "-1",
            "9427 --no-sandbox",
            "127.0.0.1:9427",
        ] {
            let error = browser_arguments(Some(OsStr::new(port)), true).unwrap_err();
            assert_eq!(error.code, "test_cdp_port", "{port}");
        }
    }

    #[test]
    fn valid_ports_bind_debugging_to_loopback() {
        for port in ["1", "9427", "65535"] {
            let args = browser_arguments(Some(OsStr::new(port)), true)
                .unwrap()
                .unwrap();
            assert!(args.contains("--remote-debugging-address=127.0.0.1"));
            assert!(args.ends_with(&format!("--remote-debugging-port={port}")));
        }
    }
}
