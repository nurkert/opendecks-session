//! GitHub releases: the JSON of the public releases API and the choice of
//! the release to offer.

use crate::version::{Version, is_newer, parse_tag, precedence};
use crate::{Error, SUMS_NAME};
use serde::Deserialize;

/// One downloadable file of a release.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    pub browser_download_url: String,
}

/// A release as the API returns it (only the fields we use).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

impl Release {
    pub fn version(&self) -> Option<Version> {
        parse_tag(&self.tag_name)
    }

    /// A pre-release: marked so on GitHub or a semver pre-release tag.
    pub fn is_prerelease(&self) -> bool {
        self.prerelease || self.version().is_some_and(|v| !v.pre.is_empty())
    }

    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }

    /// The checksum list (`SHA256SUMS.txt`).
    pub fn sums(&self) -> Option<&Asset> {
        self.asset(SUMS_NAME).or_else(|| self.asset("SHA256SUMS"))
    }
}

/// Parses the body of `GET /repos/{owner}/{repo}/releases`.
pub fn parse_releases(json: &str) -> Result<Vec<Release>, Error> {
    serde_json::from_str(json).map_err(|e| Error::Parse(format!("releases: {e}")))
}

/// Which releases may be offered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    /// Offer pre-releases too (Preferences, default off).
    pub include_prereleases: bool,
    /// A version the user chose to skip ("SKIP THIS VERSION"); newer ones
    /// are offered again.
    pub skip: Option<String>,
}

/// The newest release newer than `current` that the policy allows. Drafts,
/// tags that are not semver and the skipped version are ignored.
pub fn newest_update<'a>(releases: &'a [Release], current: &Version, policy: &Policy) -> Option<(&'a Release, Version)> {
    let skip = policy.skip.as_deref().and_then(parse_tag);
    releases
        .iter()
        .filter(|r| !r.draft)
        .filter(|r| policy.include_prereleases || !r.is_prerelease())
        .filter_map(|r| r.version().map(|v| (r, v)))
        .filter(|(_, v)| is_newer(current, v))
        .filter(|(_, v)| skip.as_ref().is_none_or(|s| precedence(s, v) != std::cmp::Ordering::Equal))
        .max_by(|a, b| precedence(&a.1, &b.1))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A trimmed copy of the real API answer for v0.0.1 (2026-11-06) plus
    /// made-up newer releases.
    pub(crate) const FIXTURE: &str = include_str!("../tests/fixtures/releases.json");

    fn ver(s: &str) -> Version {
        parse_tag(s).unwrap()
    }

    #[test]
    fn parses_the_api_answer() {
        let r = parse_releases(FIXTURE).unwrap();
        assert_eq!(r.len(), 5);
        let first = r.iter().find(|r| r.tag_name == "v0.0.1").unwrap();
        assert!(first.prerelease);
        assert_eq!(first.assets.len(), 10);
        assert_eq!(first.sums().unwrap().name, "SHA256SUMS.txt");
        assert!(first.html_url.ends_with("/releases/tag/v0.0.1"));
        assert!(parse_releases("{\"message\": \"rate limited\"}").is_err());
        assert!(parse_releases("[]").unwrap().is_empty());
    }

    #[test]
    fn picks_the_newest_stable_release() {
        let r = parse_releases(FIXTURE).unwrap();
        let (rel, v) = newest_update(&r, &ver("0.0.1"), &Policy::default()).unwrap();
        // 0.3.0-beta.1 is a pre-release, 0.4.0 a draft: 0.2.0 wins.
        assert_eq!(v, ver("0.2.0"));
        assert_eq!(rel.tag_name, "v0.2.0");
        assert!(newest_update(&r, &ver("0.2.0"), &Policy::default()).is_none());
        assert!(newest_update(&r, &ver("9.0.0"), &Policy::default()).is_none());
    }

    #[test]
    fn prereleases_only_when_opted_in() {
        let r = parse_releases(FIXTURE).unwrap();
        let p = Policy { include_prereleases: true, skip: None };
        assert_eq!(newest_update(&r, &ver("0.0.1"), &p).unwrap().1, ver("0.3.0-beta.1"));
        assert_eq!(newest_update(&r, &ver("0.2.0"), &p).unwrap().1, ver("0.3.0-beta.1"));
        // A GitHub pre-release flag alone also counts as pre-release.
        let flagged = r.iter().find(|x| x.tag_name == "v0.0.1").unwrap();
        assert!(flagged.is_prerelease());
        assert!(newest_update(std::slice::from_ref(flagged), &ver("0.0.0"), &Policy::default()).is_none());
        assert!(newest_update(std::slice::from_ref(flagged), &ver("0.0.0"), &p).is_some());
    }

    #[test]
    fn skipped_version_is_not_offered_but_newer_ones_are() {
        let r = parse_releases(FIXTURE).unwrap();
        let p = Policy { include_prereleases: false, skip: Some("0.2.0".into()) };
        assert_eq!(newest_update(&r, &ver("0.0.1"), &p).unwrap().1, ver("0.1.0"));
        let p = Policy { include_prereleases: true, skip: Some("v0.2.0".into()) };
        assert_eq!(newest_update(&r, &ver("0.0.1"), &p).unwrap().1, ver("0.3.0-beta.1"));
    }

    #[test]
    fn drafts_and_odd_tags_are_ignored() {
        let r = parse_releases(FIXTURE).unwrap();
        let p = Policy { include_prereleases: true, skip: None };
        assert!(newest_update(&r, &ver("0.0.1"), &p).is_some_and(|(r, _)| !r.draft));
        let odd = Release { tag_name: "nightly".into(), name: None, draft: false, prerelease: false, html_url: String::new(), body: None, published_at: None, assets: vec![] };
        assert!(newest_update(&[odd], &ver("0.0.1"), &p).is_none());
    }
}
