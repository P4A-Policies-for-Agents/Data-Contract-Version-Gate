// Copyright 2026 Salesforce, Inc. All rights reserved.
//! Minimal semver core parsing + comparison (no PDK imports) — unit-testable.
//! Handles MAJOR.MINOR.PATCH; a trailing pre-release/build (`-rc1`, `+meta`) is
//! ignored for comparison purposes (compared on the numeric core only).

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct Version(pub u64, pub u64, pub u64);

/// Parse a semver core `X`, `X.Y`, or `X.Y.Z` (missing parts default to 0).
/// Returns None if the numeric core is malformed.
pub fn parse(v: &str) -> Option<Version> {
    let core = v.trim();
    let core = core.split(['-', '+']).next().unwrap_or(core); // strip pre-release/build
    if core.is_empty() {
        return None;
    }
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None; // too many components
    }
    Some(Version(major, minor, patch))
}

/// True if `v` is strictly below `threshold` (both parsed). A threshold that is
/// empty/unparseable yields false (disabled).
pub fn below(v: &Version, threshold: &str) -> bool {
    match parse(threshold) {
        Some(t) => *v < t,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cores() {
        assert_eq!(parse("2.4.0"), Some(Version(2, 4, 0)));
        assert_eq!(parse("3"), Some(Version(3, 0, 0)));
        assert_eq!(parse("1.2"), Some(Version(1, 2, 0)));
        assert_eq!(parse("2.4.0-rc1"), Some(Version(2, 4, 0)));
        assert_eq!(parse("2.4.0+build.5"), Some(Version(2, 4, 0)));
    }

    #[test]
    fn rejects_bad() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("x.y"), None);
        assert_eq!(parse("1.2.3.4"), None);
    }

    #[test]
    fn ordering() {
        assert!(Version(2, 4, 0) < Version(3, 0, 0));
        assert!(Version(1, 9, 9) < Version(2, 0, 0));
        assert!(Version(2, 4, 1) > Version(2, 4, 0));
    }

    #[test]
    fn below_threshold() {
        let v = parse("2.4.0").unwrap();
        assert!(below(&v, "3.0.0"));
        assert!(!below(&v, "2.0.0"));
        assert!(!below(&v, "2.4.0")); // equal is not below
        assert!(!below(&v, "")); // disabled
        assert!(!below(&v, "garbage")); // disabled
    }
}
