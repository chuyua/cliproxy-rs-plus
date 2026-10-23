//! Home plugin sync payloads (internal/pluginstore/home_sync.go).

use std::collections::BTreeMap;

use super::auth::{ResolvedAuthConfig, validate_resolved_auth_config};
use super::manifest::Manifest;
use super::registry::normalize_install_plan;
use super::url;
use crate::go_struct;
use crate::gojson::GoTime;

pub const PLUGIN_SYNC_SCHEMA_VERSION: isize = 1;

go_struct! {
    pub struct PluginSyncRequest("pluginstore.PluginSyncRequest") {
        "schema_version" => schema_version: isize,
        "goos" => goos: String,
        "goarch" => goarch: String,
        "installed_versions" omitempty => installed_versions: BTreeMap<String, String>,
    }
}

go_struct! {
    pub struct PluginSyncItem("pluginstore.PluginSyncItem") {
        "manifest" => manifest: Manifest,
        "auth" omitempty => auth: Vec<ResolvedAuthConfig>,
    }
}

go_struct! {
    pub struct PluginSyncResponse("pluginstore.PluginSyncResponse") {
        "schema_version" => schema_version: isize,
        "expires_at" => expires_at: GoTime,
        "items" => items: Vec<PluginSyncItem>,
    }
}

impl PluginSyncResponse {
    /// Go `PluginSyncResponse.Validate`.
    pub fn validate(&self, now: chrono::DateTime<chrono::FixedOffset>) -> Result<(), String> {
        if self.schema_version != PLUGIN_SYNC_SCHEMA_VERSION {
            return Err(format!(
                "unsupported plugin sync schema_version {}",
                self.schema_version
            ));
        }
        let Some(expires_at) = self.expires_at.0 else {
            return Err("plugin sync response missing expires_at".into());
        };
        if now >= expires_at {
            return Err("plugin sync response expired".into());
        }
        let mut seen: Vec<&str> = Vec::new();
        for (index, item) in self.items.iter().enumerate() {
            item.manifest
                .validate()
                .map_err(|e| format!("plugin sync item {index}: {e}"))?;
            validate_manifest_urls(&item.manifest).map_err(|e| format!("plugin sync item {index}: {e}"))?;
            let id = item.manifest.id.trim();
            if seen.contains(&id) {
                return Err(format!(
                    "plugin sync response contains duplicate plugin {}",
                    cpa_common::gostr::quote(id)
                ));
            }
            seen.push(id);
            for (auth_index, auth) in item.auth.iter().enumerate() {
                validate_resolved_auth_config(auth)
                    .map_err(|e| format!("plugin sync item {index} auth {auth_index}: {e}"))?;
            }
        }
        Ok(())
    }
}

/// Go `validatePluginSyncManifestURLs`: synced direct installs carry pinned HTTPS
/// artifacts.
fn validate_manifest_urls(manifest: &Manifest) -> Result<(), String> {
    if manifest.install_type() != super::registry::INSTALL_TYPE_DIRECT {
        return Ok(());
    }
    let plan = normalize_install_plan(&manifest.install);
    if plan.artifacts.is_empty() {
        return Err("direct plugin sync manifest requires pinned artifacts".into());
    }
    for (index, artifact) in plan.artifacts.iter().enumerate() {
        if !url::parse(artifact.url.trim()).is_some_and(|u| u.scheme.eq_ignore_ascii_case("https")) {
            return Err(format!("direct plugin sync artifact {index} must use https"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Go internal/pluginstore/home_sync_test.go. Rust zeroes a `Secret` when it is
    //! dropped, so Go's explicit `Clear` checks have no counterpart here.

    use super::*;
    use crate::gojson;
    use crate::store::{Artifact, InstallPlan, Secret};

    const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn now() -> chrono::DateTime<chrono::FixedOffset> {
        chrono::Utc::now().fixed_offset()
    }

    fn response(url: &str, auth: Vec<ResolvedAuthConfig>) -> PluginSyncResponse {
        PluginSyncResponse {
            schema_version: PLUGIN_SYNC_SCHEMA_VERSION,
            expires_at: GoTime(Some(now() + chrono::Duration::minutes(1))),
            items: vec![PluginSyncItem {
                manifest: Manifest {
                    schema_version: 2,
                    id: "sample".into(),
                    version: "1.0.0".into(),
                    install: InstallPlan {
                        install_type: "direct".into(),
                        artifacts: vec![Artifact {
                            goos: "linux".into(),
                            goarch: "amd64".into(),
                            url: url.into(),
                            sha256: SHA.into(),
                            size: 0,
                        }],
                    },
                    ..Default::default()
                },
                auth,
            }],
        }
    }

    fn bearer(matches: &str, apply_to: &[&str]) -> Vec<ResolvedAuthConfig> {
        vec![ResolvedAuthConfig {
            matches: matches.into(),
            apply_to: apply_to.iter().map(|s| s.to_string()).collect(),
            auth_type: "bearer".into(),
            token: Secret(b"temporary-token".to_vec()),
            ..Default::default()
        }]
    }

    #[test]
    fn plugin_sync_response_validates_and_clears_resolved_auth() {
        let response = response(
            "https://downloads.example/sample.zip",
            bearer("https://downloads.example/", &[]),
        );
        response.validate(now()).unwrap();
    }

    #[test]
    fn plugin_sync_response_json_keeps_secrets_out_of_plain_text() {
        let response = PluginSyncResponse {
            schema_version: PLUGIN_SYNC_SCHEMA_VERSION,
            expires_at: GoTime(Some(now() + chrono::Duration::minutes(1))),
            items: vec![PluginSyncItem {
                auth: vec![ResolvedAuthConfig {
                    token: Secret(b"temporary-token".to_vec()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let raw = gojson::to_vec(&response);
        assert!(!String::from_utf8_lossy(&raw).contains("temporary-token"));
        let decoded: PluginSyncResponse = gojson::from_slice(&raw).unwrap();
        assert_eq!(decoded.items[0].auth[0].token.0, b"temporary-token");
    }

    #[test]
    fn plugin_sync_response_rejects_expired_plan() {
        let response = PluginSyncResponse {
            schema_version: PLUGIN_SYNC_SCHEMA_VERSION,
            expires_at: GoTime(Some(now() - chrono::Duration::seconds(1))),
            items: Vec::new(),
        };
        assert!(response.validate(now()).is_err());
    }

    #[test]
    fn plugin_sync_response_rejects_insecure_resolved_auth_match() {
        let response = response(
            "https://downloads.example/sample.zip",
            bearer("http://downloads.example/", &[]),
        );
        assert!(response.validate(now()).is_err());
    }

    #[test]
    fn plugin_sync_response_rejects_http_artifact() {
        let response = response("http://downloads.example/sample.zip", Vec::new());
        assert!(response.validate(now()).is_err());
    }

    #[test]
    fn plugin_sync_response_rejects_http_artifact_with_resolved_auth() {
        let response = response(
            "http://downloads.example/sample.zip",
            bearer("https://downloads.example/", &[crate::store::REQUEST_KIND_ARTIFACT]),
        );
        assert!(response.validate(now()).is_err());
    }

    #[test]
    fn plugin_sync_response_rejects_artifact_url_credentials() {
        let response = response("https://user:password@downloads.example/sample.zip", Vec::new());
        let error = response.validate(now()).unwrap_err();
        assert!(!error.contains("password"), "{error}");
    }
}
