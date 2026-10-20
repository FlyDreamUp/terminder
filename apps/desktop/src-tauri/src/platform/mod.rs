use terminder_session_core::model::{CoreError, Result};

pub fn validate_link(value: &str) -> Result<url::Url> {
    if value.len() > 8192 || value.chars().any(char::is_control) {
        return Err(CoreError::new("link_invalid", "链接过长或包含控制字符"));
    }
    let url = url::Url::parse(value).map_err(|_| CoreError::new("link_invalid", "链接格式无效"))?;
    if !["http", "https"].contains(&url.scheme())
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(CoreError::new(
            "link_scheme",
            "仅允许不含凭据的 HTTP/HTTPS 链接",
        ));
    }
    Ok(url)
}
#[cfg(windows)]
pub mod windows;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_never_execute_custom_schemes() {
        for value in [
            "javascript:alert(1)",
            "file:///C:/Windows",
            "ssh://example.com",
            "https://user:secret@example.com",
            "https://example.com\n",
        ] {
            assert!(validate_link(value).is_err());
        }
        assert!(validate_link("https://example.com/a?q=1").is_ok());
    }
}
