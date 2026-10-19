//! Opt-in gcl2api-plus Antigravity CLI identity.
//!
//! Python sources: `src/utils.py::build_antigravity_user_agent` and
//! `src/antigravity_cli_version.py::get_effective_version`.
//!
//! This is opt-in via credential metadata/attributes until the Python manifest
//! probe and adopted-version persistence are wired into the background runtime.
//! The existing Go-compatible User-Agent stays the default.

use cpa_core::credential::Credential;

const DEFAULT_CLI_VERSION: &str = "1.3.1";

fn version_tuple(raw: &str) -> Option<(u64, u64, u64)> {
    let mut fields = raw.split('.');
    let (Some(major), Some(minor), Some(patch), None) = (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        return None;
    };
    if [major, minor, patch]
        .iter()
        .any(|component| component.is_empty() || !component.bytes().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    Some((major.parse().ok()?, minor.parse().ok()?, patch.parse().ok()?))
}

/// Python's adopted manifest version is a floor, not a hard version pin.
/// Only known-good adopted versions are passed here; probing is handled by
/// the upstream version updater, not by the request thread.
pub fn effective_version<'a>(floor: &'a str, adopted: Option<&'a str>) -> &'a str {
    let floor = if version_tuple(floor).is_some() {
        floor
    } else {
        DEFAULT_CLI_VERSION
    };
    let Some(adopted) = adopted else {
        return floor;
    };
    if version_tuple(adopted)
        .zip(version_tuple(floor))
        .is_some_and(|(a, b)| a > b)
    {
        adopted
    } else {
        floor
    }
}

fn header_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

pub fn build_user_agent(version: &str, os_type: &str, arch: &str, auth_method: &str) -> Option<String> {
    version_tuple(version)?;
    if ![os_type, arch, auth_method].iter().all(|s| header_token(s)) {
        return None;
    }
    Some(format!(
        "antigravity/cli/{version} (aidev_client; os_type={os_type}; arch={arch}; auth_method={auth_method})"
    ))
}

fn field<'a>(c: &'a Credential, key: &str) -> Option<&'a str> {
    c.attributes
        .get(key)
        .map(String::as_str)
        .or_else(|| c.str(key))
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Use explicit `gcl2api_cli_version` or `gcl2api_cli_adopted_version` as
/// opt-in. Both are stored with the account, never used as request headers
/// verbatim, and checked for the strict `x.y.z` format.
pub fn from_credential(c: &Credential) -> Option<String> {
    let adopted = field(c, "gcl2api_cli_adopted_version");
    let floor = field(c, "gcl2api_cli_version");
    if floor.is_none() && adopted.is_none() {
        return None;
    }
    let version = effective_version(floor.unwrap_or(DEFAULT_CLI_VERSION), adopted);
    let os_type = field(c, "gcl2api_cli_os_type").unwrap_or("linux");
    let arch = field(c, "gcl2api_cli_arch").unwrap_or("amd64");
    let method = field(c, "gcl2api_cli_auth_method").unwrap_or("consumer");
    build_user_agent(version, os_type, arch, method)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    fn credential(metadata: serde_json::Value) -> Credential {
        let mut map = metadata.as_object().unwrap().clone();
        map.insert("type".into(), json!("antigravity"));
        Credential::from_file(Path::new("/mock"), &Path::new("/mock").join("account.json"), map).unwrap()
    }

    #[test]
    fn python_header_shape_and_semver_floor() {
        assert_eq!(
            build_user_agent("1.1.8", "linux", "amd64", "consumer"),
            Some("antigravity/cli/1.1.8 (aidev_client; os_type=linux; arch=amd64; auth_method=consumer)".into())
        );
        assert_eq!(effective_version("1.1.8", Some("1.3.1")), "1.3.1");
        assert_eq!(effective_version("1.3.2", Some("1.3.1")), "1.3.2");
        assert_eq!(effective_version("not-a-version", Some("1.4.0")), "1.4.0");
        assert_eq!(effective_version("1.3.1", Some("1.3.x")), "1.3.1");
        assert!(build_user_agent("1.3.1\r\nInjected: true", "linux", "amd64", "consumer").is_none());
        assert!(build_user_agent("1.3.1", "linux\r\nInjected: true", "amd64", "consumer").is_none());
    }

    #[test]
    fn explicit_account_opt_in_preserves_go_defaults() {
        let default = credential(json!({}));
        assert_eq!(from_credential(&default), None);
        assert_eq!(
            crate::antigravity_request::user_agent(&default),
            crate::antigravity_version::request_user_agent("")
        );
        let configured = credential(json!({
            "gcl2api_cli_version": "1.1.8",
            "gcl2api_cli_adopted_version": "1.3.1"
        }));
        assert_eq!(
            from_credential(&configured),
            build_user_agent("1.3.1", "linux", "amd64", "consumer")
        );
        assert_eq!(
            crate::antigravity_request::user_agent(&configured),
            "antigravity/cli/1.3.1 (aidev_client; os_type=linux; arch=amd64; auth_method=consumer)"
        );
        let invalid = credential(json!({
            "gcl2api_cli_version": "1.3.1",
            "gcl2api_cli_auth_method": "consumer\r\nBearer abc"
        }));
        assert!(from_credential(&invalid).is_none(), "do not accept header injections");
    }
}
