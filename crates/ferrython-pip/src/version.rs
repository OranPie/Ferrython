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
        let s = s.trim();
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

    /// Compare two versions. Returns Ordering.
    fn cmp_tuple(&self) -> (u32, &[u32], i32, u32, i32, u32) {
        let pre_order = match &self.pre {
            Some(PreRelease::Alpha(n)) => (-3, *n),
            Some(PreRelease::Beta(n)) => (-2, *n),
            Some(PreRelease::Rc(n)) => (-1, *n),
            None => (0, 0),
        };
        let dev_order = match self.dev {
            Some(n) => (-1, n),
            None => (0, 0),
        };
        (
            self.epoch,
            &self.release,
            pre_order.0,
            pre_order.1,
            dev_order.0,
            dev_order.1,
        )
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let a = self.cmp_tuple();
        let b = other.cmp_tuple();
        a.0.cmp(&b.0)
            .then_with(|| cmp_release(a.1, b.1))
            .then_with(|| a.2.cmp(&b.2))
            .then_with(|| a.3.cmp(&b.3))
            .then_with(|| a.4.cmp(&b.4))
            .then_with(|| a.5.cmp(&b.5))
            .then_with(|| self.post.unwrap_or(0).cmp(&other.post.unwrap_or(0)))
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
pub fn find_best_version<'a>(versions: &[&'a str], specs_str: &str) -> Option<&'a str> {
    let specs = parse_version_specs(specs_str);
    let mut candidates: Vec<(&str, Version)> = versions
        .iter()
        .filter_map(|v| Version::parse(v).map(|parsed| (*v, parsed)))
        .filter(|(_, parsed)| {
            specs.is_empty() || specs.iter().all(|s| s.matches(parsed))
        })
        .collect();

    candidates.sort_by(|a, b| b.1.cmp(&a.1));
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
}
