//! Release versions: `X.Y.Z`, with or without a leading `v` (research R1).

use std::fmt;
use std::str::FromStr;

/// A release version. Pre-release and build suffixes are refused, since releases with them
/// are never offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// The running binary's version
    pub fn current() -> Self {
        env!("CARGO_PKG_VERSION")
            .parse()
            .expect("the crate version is X.Y.Z")
    }
}

/// Not an `X.Y.Z` version
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseVersionError(String);

impl fmt::Display for ParseVersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not an X.Y.Z version: {:?}", self.0)
    }
}

impl std::error::Error for ParseVersionError {}

impl FromStr for Version {
    type Err = ParseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseVersionError(s.to_string());
        let parts: Vec<&str> = s.strip_prefix('v').unwrap_or(s).split('.').collect();
        let [major, minor, patch] = parts[..] else {
            return Err(err());
        };
        let num = |p: &str| {
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                return Err(err());
            }
            p.parse().map_err(|_| err())
        };
        Ok(Self {
            major: num(major)?,
            minor: num(minor)?,
            patch: num(patch)?,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Stored and sent as the plain `X.Y.Z` string
impl serde::Serialize for Version {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Version {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::Version;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    #[test]
    fn parses_with_and_without_v() {
        assert_eq!(v("v1.2.3"), v("1.2.3"));
        assert_eq!(v("1.2.3").to_string(), "1.2.3");
        assert_eq!(v("v0.10.0").to_string(), "0.10.0");
    }

    #[test]
    fn refuses_anything_else() {
        for bad in [
            "",
            "v",
            "1.2",
            "1.2.3-rc1",
            "1.2.3.4",
            "1..3",
            "a.b.c",
            "1.2.+3",
        ] {
            assert!(bad.parse::<Version>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn orders_numerically() {
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("0.2.1") > v("0.2.0"));
    }

    #[test]
    fn current_is_the_crate_version() {
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }
}
