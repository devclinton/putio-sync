//! Release asset names for the running platform (contracts/release-assets.md).

use crate::version::Version;

/// One of the five platforms that get archive releases
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target(&'static str);

impl Target {
    /// The platform this binary was built for, or `None` where no archive is published
    /// (Intel macOS included)
    pub fn current() -> Option<Self> {
        Self::for_platform(std::env::consts::OS, std::env::consts::ARCH)
    }

    fn for_platform(os: &str, arch: &str) -> Option<Self> {
        let name = match (os, arch) {
            ("linux", "x86_64") => "linux-x86_64",
            ("linux", "aarch64") => "linux-aarch64",
            ("macos", "aarch64") => "macos-arm64",
            ("windows", "x86_64") => "windows-x64",
            ("windows", "aarch64") => "windows-arm64",
            _ => return None,
        };
        Some(Self(name))
    }

    /// The target as asset names spell it
    pub fn as_str(&self) -> &'static str {
        self.0
    }

    /// The xz-compressed bare binary
    pub fn full(&self, version: Version) -> String {
        format!("putio-sync-{version}-{}.xz", self.0)
    }

    /// The bsdiff patch from `from` to `to`, xz-compressed
    pub fn patch(&self, from: Version, to: Version) -> String {
        format!("putio-sync-{from}-to-{to}-{}.bsdiff.xz", self.0)
    }

    /// The `SHA256SUMS` line for the uncompressed binary (no file has this name)
    pub fn bin(&self, version: Version) -> String {
        format!("putio-sync-{version}-{}.bin", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::Target;

    #[test]
    fn maps_the_five_archive_platforms() {
        let name = |os, arch| Target::for_platform(os, arch).map(|t| t.as_str());
        assert_eq!(name("linux", "x86_64"), Some("linux-x86_64"));
        assert_eq!(name("linux", "aarch64"), Some("linux-aarch64"));
        assert_eq!(name("macos", "aarch64"), Some("macos-arm64"));
        assert_eq!(name("windows", "x86_64"), Some("windows-x64"));
        assert_eq!(name("windows", "aarch64"), Some("windows-arm64"));
        assert_eq!(name("macos", "x86_64"), None);
        assert_eq!(name("freebsd", "x86_64"), None);
        assert_eq!(name("linux", "arm"), None);
    }

    #[test]
    fn asset_names() {
        let t = Target::for_platform("linux", "x86_64").unwrap();
        let (a, b) = ("0.2.0".parse().unwrap(), "0.3.0".parse().unwrap());
        assert_eq!(t.full(b), "putio-sync-0.3.0-linux-x86_64.xz");
        assert_eq!(
            t.patch(a, b),
            "putio-sync-0.2.0-to-0.3.0-linux-x86_64.bsdiff.xz"
        );
        assert_eq!(t.bin(b), "putio-sync-0.3.0-linux-x86_64.bin");
    }
}
