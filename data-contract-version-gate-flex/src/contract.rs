// Copyright 2026 Salesforce, Inc. All rights reserved.
//! Pure data-contract version-gate decision logic (no PDK imports).

use crate::semver::{self, Version};

/// Resolved thresholds for a contract (per-contract override merged over global).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Rules {
    pub blocked_below: String,
    pub deprecated_below: String,
    pub sunset: String,
}

impl Rules {
    pub fn is_governed(&self) -> bool {
        !self.blocked_below.trim().is_empty() || !self.deprecated_below.trim().is_empty()
    }
}

/// Outcome of gating a declared version.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// Not governed (no thresholds configured for this contract) — allow.
    NotGoverned,
    /// Allowed and current.
    Allow,
    /// Allowed but the version is deprecated (emit Deprecation/Sunset).
    Deprecated,
    /// Blocked — below the blocked threshold (reject + upgrade hint).
    Blocked,
    /// Version missing or unparseable for a governed contract.
    Invalid,
}

/// Merge a per-contract override (opt fields) over the global fallback.
pub fn resolve_rules(
    global_blocked: &str,
    global_deprecated: &str,
    per_blocked: Option<&str>,
    per_deprecated: Option<&str>,
    per_sunset: Option<&str>,
) -> Rules {
    let pick = |ov: Option<&str>, g: &str| -> String {
        match ov {
            Some(s) if !s.trim().is_empty() => s.to_string(),
            _ => g.to_string(),
        }
    };
    Rules {
        blocked_below: pick(per_blocked, global_blocked),
        deprecated_below: pick(per_deprecated, global_deprecated),
        sunset: per_sunset.unwrap_or("").to_string(),
    }
}

/// Decide the outcome for a declared `version` under `rules`.
pub fn evaluate(rules: &Rules, version: Option<&str>) -> Decision {
    if !rules.is_governed() {
        return Decision::NotGoverned;
    }
    let v: Version = match version.and_then(semver::parse) {
        Some(v) => v,
        None => return Decision::Invalid,
    };
    if semver::below(&v, &rules.blocked_below) {
        return Decision::Blocked;
    }
    if semver::below(&v, &rules.deprecated_below) {
        return Decision::Deprecated;
    }
    Decision::Allow
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(b: &str, d: &str) -> Rules {
        Rules { blocked_below: b.into(), deprecated_below: d.into(), sunset: String::new() }
    }

    #[test]
    fn not_governed_when_no_thresholds() {
        assert_eq!(evaluate(&rules("", ""), Some("2.4.0")), Decision::NotGoverned);
    }

    #[test]
    fn blocks_below_blocked() {
        assert_eq!(evaluate(&rules("1.0.0", "3.0.0"), Some("0.9.0")), Decision::Blocked);
    }

    #[test]
    fn deprecates_between_blocked_and_deprecated() {
        assert_eq!(evaluate(&rules("1.0.0", "3.0.0"), Some("2.4.0")), Decision::Deprecated);
    }

    #[test]
    fn allows_at_or_above_deprecated() {
        assert_eq!(evaluate(&rules("1.0.0", "3.0.0"), Some("3.0.0")), Decision::Allow);
        assert_eq!(evaluate(&rules("1.0.0", "3.0.0"), Some("3.5.1")), Decision::Allow);
    }

    #[test]
    fn invalid_when_missing_or_bad_and_governed() {
        assert_eq!(evaluate(&rules("1.0.0", ""), None), Decision::Invalid);
        assert_eq!(evaluate(&rules("1.0.0", ""), Some("not-a-version")), Decision::Invalid);
    }

    #[test]
    fn resolve_prefers_per_contract() {
        let r = resolve_rules("1.0.0", "2.0.0", Some("1.5.0"), None, Some("Wed, 01 Jan 2027 00:00:00 GMT"));
        assert_eq!(r.blocked_below, "1.5.0"); // per-contract override
        assert_eq!(r.deprecated_below, "2.0.0"); // global fallback
        assert_eq!(r.sunset, "Wed, 01 Jan 2027 00:00:00 GMT");
    }
}
