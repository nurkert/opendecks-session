//! Version tags and their order (semver).

pub use semver::Version;
use std::cmp::Ordering;

/// Parses a release tag ("v1.2.3", "1.2.3-beta.1"); `None` for tags that
/// are not semver.
pub fn parse_tag(tag: &str) -> Option<Version> {
    let t = tag.trim();
    let t = t.strip_prefix(['v', 'V']).unwrap_or(t);
    Version::parse(t).ok()
}

/// Semver precedence: build metadata is ignored, a pre-release sorts
/// before its release (1.0.0-rc.1 < 1.0.0).
pub fn precedence(a: &Version, b: &Version) -> Ordering {
    (a.major, a.minor, a.patch)
        .cmp(&(b.major, b.minor, b.patch))
        .then_with(|| match (a.pre.is_empty(), b.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => a.pre.cmp(&b.pre),
        })
}

/// True when `candidate` is newer than `current`.
pub fn is_newer(current: &Version, candidate: &Version) -> bool {
    precedence(candidate, current) == Ordering::Greater
}

/// The running app's version.
pub fn current() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is semver")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        parse_tag(s).unwrap()
    }

    #[test]
    fn tags_parse_with_or_without_v() {
        assert_eq!(v("v1.2.3"), Version::new(1, 2, 3));
        assert_eq!(v("1.2.3"), Version::new(1, 2, 3));
        assert_eq!(v(" V0.0.1 "), Version::new(0, 0, 1));
        assert!(!v("v2.0.0-beta.1").pre.is_empty());
        assert!(parse_tag("nightly").is_none());
        assert!(parse_tag("v1.2").is_none());
        assert!(parse_tag("").is_none());
    }

    #[test]
    fn semver_order() {
        assert!(is_newer(&v("0.0.1"), &v("0.0.2")));
        assert!(is_newer(&v("0.9.9"), &v("0.10.0")));
        assert!(is_newer(&v("1.9.0"), &v("2.0.0")));
        assert!(!is_newer(&v("1.0.0"), &v("1.0.0")));
        assert!(!is_newer(&v("1.0.1"), &v("1.0.0")));
        // Pre-releases sort before their release.
        assert!(is_newer(&v("1.0.0-rc.1"), &v("1.0.0")));
        assert!(!is_newer(&v("1.0.0"), &v("1.0.0-rc.2")));
        assert!(is_newer(&v("1.0.0-beta.2"), &v("1.0.0-beta.10")));
        assert!(is_newer(&v("1.0.0-alpha"), &v("1.0.0-beta")));
        assert!(is_newer(&v("0.9.0"), &v("1.0.0-rc.1")));
        // Build metadata does not count.
        assert!(!is_newer(&v("1.0.0+a"), &v("1.0.0+b")));
    }

    #[test]
    fn own_version_parses() {
        let _ = current();
    }
}
