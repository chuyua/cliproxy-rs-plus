//! gcl2api-plus Antigravity HTTP 403 decision rules.
//!
//! Ported from `src/antigravity_error_classifier.py` (custom branch 776430a).
//! This classifier never disables accounts on an ambiguous 403. The Rust gateway
//! currently consumes its model-specific cooldown decisions; the separate
//! health-probe and rebind workflow is not yet connected.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    ProxyDrift,
    ProxyFailed,
    ValidationRequired,
    GeoBlocked,
    ModelForbidden,
    AccountForbidden,
    Unknown403,
}

impl Category {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProxyDrift => "proxy_drift",
            Self::ProxyFailed => "proxy_failed",
            Self::ValidationRequired => "validation_required",
            Self::GeoBlocked => "geo_blocked",
            Self::ModelForbidden => "model_forbidden",
            Self::AccountForbidden => "account_forbidden",
            Self::Unknown403 => "unknown_403",
        }
    }

    /// gcl2api-plus: MODEL_FORBIDDEN_COOLDOWN_SECONDS and
    /// VALIDATION_REQUIRED_COOLDOWN_SECONDS, not Go's generic 30-minute 403.
    pub const fn model_cooldown_seconds(self) -> Option<u64> {
        match self {
            Self::ModelForbidden => Some(300),
            Self::ValidationRequired => Some(600),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub category: Category,
    pub disable_account: bool,
    pub invalidate_binding: bool,
    pub cooldown_model: bool,
    pub retry_after_rebind: bool,
}

impl Decision {
    const fn new(category: Category) -> Self {
        Self {
            category,
            disable_account: false,
            invalidate_binding: false,
            cooldown_model: false,
            retry_after_rebind: false,
        }
    }

    const fn invalid_binding(category: Category) -> Self {
        Self {
            invalidate_binding: true,
            retry_after_rebind: true,
            ..Self::new(category)
        }
    }

    const fn model_cooldown(category: Category) -> Self {
        Self {
            cooldown_model: true,
            ..Self::new(category)
        }
    }
}

fn contains_any(text: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| text.contains(marker))
}

const LOCATION: &[&str] = &[
    "not currently available in your location",
    "not available in your location",
    "unsupported location",
    "unsupported country",
    "region is not supported",
    "country is not supported",
    "location restriction",
    "geo restriction",
    "geographic restriction",
];
const MODEL: &[&str] = &[
    "model is not available",
    "model is not permitted",
    "model access",
    "does not have access to model",
    "not allowed to use model",
];
const VALIDATION: &[&str] = &["validation_required", "verify your account", "validation_url"];
const ACCOUNT_PERMISSION: &[&str] = &[
    "authenticated account does not have permission",
    "account does not have permission",
    "account is not permitted",
    "account has been disabled",
    "account has been suspended",
    "access has been revoked",
];

/// Exact precedence from Python:
/// proxy binding -> validation -> explicit region -> explicit model ->
/// cached region -> confirmed account permission -> unknown.
///
/// Cached account health is optional and must not be fabricated if no probe ran.
pub fn classify(error_text: &str, binding_status: Option<&str>, eligibility_status: Option<&str>) -> Decision {
    let binding = binding_status.unwrap_or("unchecked").to_ascii_lowercase();
    let eligibility = eligibility_status.unwrap_or("unchecked").to_ascii_lowercase();
    let normalized = error_text.to_lowercase();
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");

    match binding.as_str() {
        "proxy_drift" => return Decision::invalid_binding(Category::ProxyDrift),
        "proxy_failed" => return Decision::invalid_binding(Category::ProxyFailed),
        _ => {}
    }
    if contains_any(&normalized, VALIDATION) {
        return Decision::model_cooldown(Category::ValidationRequired);
    }
    if contains_any(&normalized, LOCATION) {
        return Decision::invalid_binding(Category::GeoBlocked);
    }
    if contains_any(&normalized, MODEL)
        || (normalized.contains("model ")
            && (normalized.contains("not available") || normalized.contains("not permitted")))
    {
        return Decision::model_cooldown(Category::ModelForbidden);
    }
    if eligibility == "geo_blocked" {
        return Decision::invalid_binding(Category::GeoBlocked);
    }
    if binding == "healthy" && eligibility == "eligible" && contains_any(&normalized, ACCOUNT_PERMISSION) {
        return Decision {
            disable_account: true,
            ..Decision::new(Category::AccountForbidden)
        };
    }
    Decision::new(Category::Unknown403)
}

#[cfg(test)]
mod tests {
    use super::{Category, classify};

    #[test]
    fn python_reference_cases() {
        let geo = classify("permission denied", Some("healthy"), Some("geo_blocked"));
        assert_eq!(geo.category, Category::GeoBlocked);
        assert!(!geo.disable_account);
        assert!(geo.invalidate_binding && geo.retry_after_rebind);

        let model = classify(
            "Model gemini-image is not available for this account.",
            Some("healthy"),
            Some("geo_blocked"),
        );
        assert_eq!(model.category, Category::ModelForbidden);
        assert!(model.cooldown_model && !model.disable_account);

        let proxy = classify("forbidden", Some("proxy_drift"), Some("eligible"));
        assert_eq!(proxy.category, Category::ProxyDrift);
        assert!(proxy.invalidate_binding && !proxy.disable_account);

        let region = classify("Your account is not currently available in your location.", None, None);
        assert_eq!(region.category, Category::GeoBlocked);

        let account = classify(
            "The authenticated account does not have permission to use Antigravity.",
            Some("healthy"),
            Some("eligible"),
        );
        assert_eq!(account.category, Category::AccountForbidden);
        assert!(account.disable_account);

        let model_perm = classify(
            "Model gemini-test is not available or permitted for this account.",
            Some("healthy"),
            Some("eligible"),
        );
        assert_eq!(model_perm.category, Category::ModelForbidden);
        assert_eq!(model_perm.category.model_cooldown_seconds(), Some(300));

        let unknown = classify("Forbidden", None, None);
        assert_eq!(unknown.category, Category::Unknown403);
        assert!(!unknown.disable_account);
        assert!(!unknown.invalidate_binding);
        assert!(!unknown.retry_after_rebind);

        for validation in [
            "Account validation is required: VALIDATION_REQUIRED",
            "Please verify your account to continue",
            "Please complete verification at validation_url",
        ] {
            let decision = classify(validation, Some("healthy"), Some("eligible"));
            assert_eq!(decision.category, Category::ValidationRequired);
            assert!(!decision.disable_account && !decision.invalidate_binding);
            assert!(decision.cooldown_model);
            assert_eq!(decision.category.model_cooldown_seconds(), Some(600));
        }
    }

    #[test]
    fn precedence_and_conservative_guards() {
        assert_eq!(
            classify(
                "validation_required model is not available",
                Some("proxy_failed"),
                Some("geo_blocked"),
            )
            .category,
            Category::ProxyFailed
        );
        assert_eq!(
            classify(
                "validation_required model is not available",
                Some("healthy"),
                Some("geo_blocked"),
            )
            .category,
            Category::ValidationRequired
        );
        assert_eq!(
            classify("account has been suspended", Some("unchecked"), Some("eligible"),).category,
            Category::Unknown403
        );
        assert_eq!(
            classify("model \n is not permitted", None, None,).category,
            Category::ModelForbidden
        );
        assert_eq!(Category::Unknown403.as_str(), "unknown_403");
    }
}
