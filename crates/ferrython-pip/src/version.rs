//! PEP 440 version specifier parsing and matching.

/// A parsed version (simplified PEP 440).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub epoch: u32,
    pub release: Vec<u32>,
    pub pre: Option<PreRelease>,
    pub post: Option<u32>,
    pub dev: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreRelease {
    Alpha(u32),
    Beta(u32),
    Rc(u32),
}

impl Version {
    /// Parse a version string like "1.2.3", "2.0.0a1", "3.1.0.post1".
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('v');
        if s.is_empty() {
            return None;
        }

        let (epoch, rest) = if let Some(pos) = s.find('!') {
            (s[..pos].parse::<u32>().unwrap_or(0), &s[pos + 1..])
        } else {
            (0, s)
        };

        // Split off pre/post/dev suffixes
        let (release_str, pre, post, dev) = parse_suffixes(rest);

        let release: Vec<u32> = release_str
            .split('.')
            .filter_map(|p| p.parse().ok())
            .collect();

        if release.is_empty() {
            return None;
        }

        Some(Version { epoch, release, pre, post, dev })
    }

    /// PEP 440 pre-release comparison key.
    ///
    /// Dev-only releases (no pre, no post, has dev) sort below all pre-releases.
    /// Pre-releases sort below final. Final is the highest pre-phase.
    fn pre_cmp_key(&self) -> (i32, u32) {
        match (&self.pre, &self.post, &self.dev) {
            // dev-only release (e.g. 1.0.dev0): below all pre-releases
            (None, None, Some(_)) => (-4, 0),
            (Some(PreRelease::Alpha(n)), _, _) => (-3, *n),
            (Some(PreRelease::Beta(n)), _, _) => (-2, *n),
            (Some(PreRelease::Rc(n)), _, _) => (-1, *n),
            // final release (no pre-release tag)
            (None, _, _) => (0, 0),
        }
    }

    /// PEP 440 post-release comparison key.
    ///
    /// No post-release sorts below post0.
    fn post_cmp_key(&self) -> i64 {
        match self.post {
            None => -1,
            Some(n) => n as i64,
        }
    }

    /// PEP 440 dev-release comparison key.
    ///
    /// Dev versions sort below their non-dev counterpart. No dev = final.
    fn dev_cmp_key(&self) -> i64 {
        match self.dev {
            Some(n) => n as i64,
            None => i64::MAX,
        }
    }

    /// Check if this is a pre-release or dev version.
    pub fn is_prerelease(&self) -> bool {
        self.pre.is_some() || self.dev.is_some()
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.epoch.cmp(&other.epoch)
            .then_with(|| cmp_release(&self.release, &other.release))
            .then_with(|| self.pre_cmp_key().cmp(&other.pre_cmp_key()))
            .then_with(|| self.post_cmp_key().cmp(&other.post_cmp_key()))
            .then_with(|| self.dev_cmp_key().cmp(&other.dev_cmp_key()))
    }
}

fn cmp_release(a: &[u32], b: &[u32]) -> std::cmp::Ordering {
    let max_len = a.len().max(b.len());
    for i in 0..max_len {
        let va = a.get(i).copied().unwrap_or(0);
        let vb = b.get(i).copied().unwrap_or(0);
        match va.cmp(&vb) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.epoch != 0 {
            write!(f, "{}!", self.epoch)?;
        }
        let parts: Vec<String> = self.release.iter().map(|n| n.to_string()).collect();
        write!(f, "{}", parts.join("."))?;
        match &self.pre {
            Some(PreRelease::Alpha(n)) => write!(f, "a{}", n)?,
            Some(PreRelease::Beta(n)) => write!(f, "b{}", n)?,
            Some(PreRelease::Rc(n)) => write!(f, "rc{}", n)?,
            None => {}
        }
        if let Some(n) = self.post {
            write!(f, ".post{}", n)?;
        }
        if let Some(n) = self.dev {
            write!(f, ".dev{}", n)?;
        }
        Ok(())
    }
}

fn parse_suffixes(s: &str) -> (&str, Option<PreRelease>, Option<u32>, Option<u32>) {
    let s_lower = s.to_lowercase();
    let bytes = s_lower.as_bytes();

    // Find where the release part ends and suffixes begin
    let mut end = s.len();
    let mut pre = None;
    let mut post = None;
    let mut dev = None;

    // Look for dev
    if let Some(pos) = s_lower.find(".dev") {
        dev = Some(s_lower[pos + 4..].parse().unwrap_or(0));
        end = end.min(pos);
    } else if let Some(pos) = s_lower.find("dev") {
        if pos > 0 && !bytes[pos - 1].is_ascii_alphabetic() {
            dev = Some(s_lower[pos + 3..end].trim_start_matches('.').parse().unwrap_or(0));
            end = end.min(pos);
        }
    }

    // Look for post
    let check = &s_lower[..end];
    if let Some(pos) = check.find(".post") {
        post = Some(check[pos + 5..].parse().unwrap_or(0));
        end = pos;
    } else if let Some(pos) = check.find("post") {
        if pos > 0 {
            post = Some(check[pos + 4..].parse().unwrap_or(0));
            end = pos;
        }
    }

    // Look for pre-release
    let check = &s_lower[..end];
    let patterns: &[(&str, u8)] = &[
        ("alpha", 1), ("beta", 2), ("rc", 3),
        ("a", 1), ("b", 2), ("c", 3),
    ];
    for &(tag, kind) in patterns {
        if let Some(pos) = check.rfind(tag) {
            if pos > 0 && check.as_bytes()[pos - 1].is_ascii_digit() {
                let num_str = &check[pos + tag.len()..];
                let n = num_str.parse().unwrap_or(0);
                pre = Some(match kind {
                    1 => PreRelease::Alpha(n),
                    2 => PreRelease::Beta(n),
                    _ => PreRelease::Rc(n),
                });
                end = pos;
                break;
            }
        }
    }

    (&s[..end], pre, post, dev)
}

/// A version specifier like ">=1.0", "==2.3.*", "~=1.4.2".
#[derive(Debug, Clone)]
pub struct VersionSpec {
    pub op: SpecOp,
    pub version: Version,
    pub wildcard: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpecOp {
    Eq,       // ==
    Ne,       // !=
    Ge,       // >=
    Le,       // <=
    Gt,       // >
    Lt,       // <
    Compat,   // ~=
}

impl VersionSpec {
    /// Parse a single specifier like ">=1.0.0" or "~=2.0".
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (op, rest) = if s.starts_with("~=") {
            (SpecOp::Compat, s[2..].trim())
        } else if s.starts_with("==") {
            (SpecOp::Eq, s[2..].trim())
        } else if s.starts_with("!=") {
            (SpecOp::Ne, s[2..].trim())
        } else if s.starts_with(">=") {
            (SpecOp::Ge, s[2..].trim())
        } else if s.starts_with("<=") {
            (SpecOp::Le, s[2..].trim())
        } else if s.starts_with('>') {
            (SpecOp::Gt, s[1..].trim())
        } else if s.starts_with('<') {
            (SpecOp::Lt, s[1..].trim())
        } else {
            // Bare version → treat as ==
            (SpecOp::Eq, s)
        };

        let wildcard = rest.ends_with(".*");
        let ver_str = if wildcard { &rest[..rest.len() - 2] } else { rest };
        let version = Version::parse(ver_str)?;

        Some(VersionSpec { op, version, wildcard })
    }

    /// Check if a candidate version satisfies this specifier.
    pub fn matches(&self, candidate: &Version) -> bool {
        match self.op {
            SpecOp::Eq => {
                if self.wildcard {
                    // ==1.0.* means 1.0.anything
                    let prefix = &self.version.release;
                    candidate.release.len() >= prefix.len()
                        && candidate.release[..prefix.len()] == *prefix
                } else {
                    candidate == &self.version
                }
            }
            SpecOp::Ne => {
                if self.wildcard {
                    let prefix = &self.version.release;
                    !(candidate.release.len() >= prefix.len()
                        && candidate.release[..prefix.len()] == *prefix)
                } else {
                    candidate != &self.version
                }
            }
            SpecOp::Ge => candidate >= &self.version,
            SpecOp::Le => candidate <= &self.version,
            SpecOp::Gt => candidate > &self.version,
            SpecOp::Lt => candidate < &self.version,
            SpecOp::Compat => {
                // ~=X.Y is equivalent to >=X.Y, ==X.*
                if candidate < &self.version {
                    return false;
                }
                // Check prefix match (all but last segment)
                let prefix_len = self.version.release.len().saturating_sub(1).max(1);
                candidate.release.len() >= prefix_len
                    && candidate.release[..prefix_len] == self.version.release[..prefix_len]
            }
        }
    }
}

impl std::fmt::Display for VersionSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let op_str = match self.op {
            SpecOp::Eq => "==",
            SpecOp::Ne => "!=",
            SpecOp::Ge => ">=",
            SpecOp::Le => "<=",
            SpecOp::Gt => ">",
            SpecOp::Lt => "<",
            SpecOp::Compat => "~=",
        };
        write!(f, "{}{}", op_str, self.version)?;
        if self.wildcard {
            write!(f, ".*")?;
        }
        Ok(())
    }
}

/// Parse a full version requirement string like ">=1.0,<2.0" or "~=1.4.2".
pub fn parse_version_specs(s: &str) -> Vec<VersionSpec> {
    s.split(',')
        .filter_map(|part| VersionSpec::parse(part.trim()))
        .collect()
}

/// Check if a version string satisfies all specifiers in a requirement string.
pub fn version_matches(version_str: &str, specs_str: &str) -> bool {
    let version = match Version::parse(version_str) {
        Some(v) => v,
        None => return false,
    };
    let specs = parse_version_specs(specs_str);
    if specs.is_empty() {
        return true;
    }
    specs.iter().all(|spec| spec.matches(&version))
}

/// From a list of version strings, find the best (highest) that satisfies all specs.
/// Excludes pre-release and dev versions unless explicitly matched by a spec.
pub fn find_best_version<'a>(versions: &[&'a str], specs_str: &str) -> Option<&'a str> {
    let specs = parse_version_specs(specs_str);
    let allow_pre = specs.iter().any(|s| s.version.pre.is_some() || s.version.dev.is_some());

    let mut candidates: Vec<(&str, Version)> = versions
        .iter()
        .filter_map(|v| Version::parse(v).map(|parsed| (*v, parsed)))
        .filter(|(_, parsed)| {
            // Skip pre-release/dev versions unless explicitly requested
            if !allow_pre && parsed.is_prerelease() {
                return false;
            }
            specs.is_empty() || specs.iter().all(|s| s.matches(parsed))
        })
        .collect();

    candidates.sort_by(|a, b| b.1.cmp(&a.1));

    // If no stable candidates found, fall back to including pre-releases
    if candidates.is_empty() && !allow_pre {
        candidates = versions
            .iter()
            .filter_map(|v| Version::parse(v).map(|parsed| (*v, parsed)))
            .filter(|(_, parsed)| specs.is_empty() || specs.iter().all(|s| s.matches(parsed)))
            .collect();
        candidates.sort_by(|a, b| b.1.cmp(&a.1));
    }

    candidates.first().map(|(s, _)| *s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parse() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(v.release, vec![1, 2, 3]);
        assert_eq!(v.epoch, 0);

        let v = Version::parse("2!1.0").unwrap();
        assert_eq!(v.epoch, 2);
        assert_eq!(v.release, vec![1, 0]);
    }

    #[test]
    fn test_version_ordering() {
        assert!(Version::parse("1.0").unwrap() < Version::parse("1.1").unwrap());
        assert!(Version::parse("1.0").unwrap() < Version::parse("1.0.1").unwrap());
        assert!(Version::parse("2.0").unwrap() > Version::parse("1.99").unwrap());
    }

    #[test]
    fn test_spec_ge() {
        let spec = VersionSpec::parse(">=1.0").unwrap();
        assert!(spec.matches(&Version::parse("1.0").unwrap()));
        assert!(spec.matches(&Version::parse("1.1").unwrap()));
        assert!(!spec.matches(&Version::parse("0.9").unwrap()));
    }

    #[test]
    fn test_spec_compat() {
        let spec = VersionSpec::parse("~=1.4.2").unwrap();
        assert!(spec.matches(&Version::parse("1.4.2").unwrap()));
        assert!(spec.matches(&Version::parse("1.4.5").unwrap()));
        assert!(!spec.matches(&Version::parse("1.5.0").unwrap()));
        assert!(!spec.matches(&Version::parse("1.4.1").unwrap()));
    }

    #[test]
    fn test_version_matches() {
        assert!(version_matches("1.5.0", ">=1.0,<2.0"));
        assert!(!version_matches("2.0.0", ">=1.0,<2.0"));
        assert!(version_matches("1.0.0", "==1.0.*"));
        assert!(version_matches("1.0.5", "==1.0.*"));
        assert!(!version_matches("1.1.0", "==1.0.*"));
    }

    #[test]
    fn test_find_best() {
        let versions = vec!["1.0.0", "1.1.0", "1.2.0", "2.0.0"];
        assert_eq!(find_best_version(&versions, ">=1.0,<2.0"), Some("1.2.0"));
        assert_eq!(find_best_version(&versions, ">=2.0"), Some("2.0.0"));
    }

    #[test]
    fn test_version_spec_display() {
        let spec = VersionSpec::parse(">=1.0.0").unwrap();
        assert_eq!(format!("{}", spec), ">=1.0.0");

        let spec = VersionSpec::parse("~=2.3").unwrap();
        assert_eq!(format!("{}", spec), "~=2.3");

        let spec = VersionSpec::parse("==1.0.*").unwrap();
        assert_eq!(format!("{}", spec), "==1.0.*");
    }

    #[test]
    fn test_ne_spec() {
        let spec = VersionSpec::parse("!=1.5.0").unwrap();
        assert!(!spec.matches(&Version::parse("1.5.0").unwrap()));
        assert!(spec.matches(&Version::parse("1.5.1").unwrap()));
        assert!(spec.matches(&Version::parse("1.4.0").unwrap()));
    }

    #[test]
    fn test_ne_wildcard() {
        let spec = VersionSpec::parse("!=1.5.*").unwrap();
        assert!(!spec.matches(&Version::parse("1.5.0").unwrap()));
        assert!(!spec.matches(&Version::parse("1.5.9").unwrap()));
        assert!(spec.matches(&Version::parse("1.6.0").unwrap()));
        assert!(spec.matches(&Version::parse("1.4.0").unwrap()));
    }

    #[test]
    fn test_lt_gt_spec() {
        assert!(version_matches("1.0.0", ">0.9"));
        assert!(!version_matches("1.0.0", ">1.0.0"));
        assert!(version_matches("1.0.0", "<1.0.1"));
        assert!(!version_matches("1.0.0", "<1.0.0"));
    }

    #[test]
    fn test_combined_range_specs() {
        assert!(version_matches("1.5.0", ">=1.0,<2.0,!=1.3.0"));
        assert!(!version_matches("1.3.0", ">=1.0,<2.0,!=1.3.0"));
        assert!(!version_matches("0.9.0", ">=1.0,<2.0,!=1.3.0"));
        assert!(!version_matches("2.0.0", ">=1.0,<2.0,!=1.3.0"));
    }

    #[test]
    fn test_pre_release_ordering() {
        assert!(Version::parse("1.0a1").unwrap() < Version::parse("1.0b1").unwrap());
        assert!(Version::parse("1.0b1").unwrap() < Version::parse("1.0rc1").unwrap());
        assert!(Version::parse("1.0rc1").unwrap() < Version::parse("1.0").unwrap());
    }

    #[test]
    fn test_find_best_excludes_prerelease() {
        let versions = vec!["1.0.0", "1.1.0", "2.0.0a1", "2.0.0b1"];
        // Should pick 1.1.0, not 2.0.0a1
        assert_eq!(find_best_version(&versions, ">=1.0"), Some("1.1.0"));
    }

    #[test]
    fn test_find_best_includes_prerelease_when_explicit() {
        let versions = vec!["1.0.0", "2.0.0a1", "2.0.0b1"];
        // Spec includes pre-release version → allow pre-releases
        assert_eq!(find_best_version(&versions, ">=2.0.0a1"), Some("2.0.0b1"));
    }

    #[test]
    fn test_epoch_ordering() {
        assert!(Version::parse("2!1.0").unwrap() > Version::parse("99.0").unwrap());
    }

    #[test]
    fn test_post_release() {
        assert!(Version::parse("1.0.post1").unwrap() > Version::parse("1.0").unwrap());
        assert!(Version::parse("1.0.post2").unwrap() > Version::parse("1.0.post1").unwrap());
    }

    #[test]
    fn test_dev_release_ordering() {
        // dev releases are below their final counterparts
        assert!(Version::parse("1.0.dev0").unwrap() < Version::parse("1.0").unwrap());
        assert!(Version::parse("1.0.dev0").unwrap() < Version::parse("1.0a1").unwrap());
        assert!(Version::parse("1.0.dev1").unwrap() < Version::parse("1.0.dev2").unwrap());
    }

    #[test]
    fn test_pep440_full_ordering() {
        // PEP 440 full ordering:
        // 1.0.dev0 < 1.0a1 < 1.0a2 < 1.0b1 < 1.0rc1 < 1.0 < 1.0.post1.dev0 < 1.0.post1
        let versions = vec![
            "1.0.post1",
            "1.0.post1.dev0",
            "1.0",
            "1.0rc1",
            "1.0b1",
            "1.0a2",
            "1.0a1",
            "1.0.dev0",
        ];
        let mut parsed: Vec<Version> = versions.iter()
            .map(|v| Version::parse(v).unwrap())
            .collect();
        parsed.sort();
        let sorted: Vec<String> = parsed.iter().map(|v| format!("{}", v)).collect();
        assert_eq!(sorted, vec![
            "1.0.dev0", "1.0a1", "1.0a2", "1.0b1", "1.0rc1",
            "1.0", "1.0.post1.dev0", "1.0.post1"
        ]);
    }

    #[test]
    fn test_post_dev_ordering() {
        // post1.dev0 is between final and post1
        assert!(Version::parse("1.0").unwrap() < Version::parse("1.0.post1.dev0").unwrap());
        assert!(Version::parse("1.0.post1.dev0").unwrap() < Version::parse("1.0.post1").unwrap());
    }

    #[test]
    fn test_compat_release_operator() {
        // ~=1.4.2 means >=1.4.2, ==1.4.*
        assert!(version_matches("1.4.2", "~=1.4.2"));
        assert!(version_matches("1.4.5", "~=1.4.2"));
        assert!(!version_matches("1.5.0", "~=1.4.2"));
        assert!(!version_matches("1.3.9", "~=1.4.2"));

        // ~=2.2 means >=2.2, ==2.*
        assert!(version_matches("2.2", "~=2.2"));
        assert!(version_matches("2.9", "~=2.2"));
        assert!(!version_matches("3.0", "~=2.2"));
        assert!(!version_matches("2.1", "~=2.2"));
    }

    #[test]
    fn test_wildcard_matching() {
        assert!(version_matches("1.0.0", "==1.0.*"));
        assert!(version_matches("1.0.99", "==1.0.*"));
        assert!(!version_matches("1.1.0", "==1.0.*"));

        // !=1.5.* excludes all 1.5.x
        assert!(!version_matches("1.5.0", "!=1.5.*"));
        assert!(!version_matches("1.5.9", "!=1.5.*"));
        assert!(version_matches("1.6.0", "!=1.5.*"));
    }

    #[test]
    fn test_version_with_v_prefix() {
        let v = Version::parse("v1.2.3").unwrap();
        assert_eq!(v.release, vec![1, 2, 3]);
    }

    #[test]
    fn test_is_prerelease() {
        assert!(Version::parse("1.0a1").unwrap().is_prerelease());
        assert!(Version::parse("1.0.dev0").unwrap().is_prerelease());
        assert!(Version::parse("1.0b1").unwrap().is_prerelease());
        assert!(Version::parse("1.0rc1").unwrap().is_prerelease());
        assert!(!Version::parse("1.0").unwrap().is_prerelease());
        assert!(!Version::parse("1.0.post1").unwrap().is_prerelease());
    }
}
