//! Update channels: which manifests a build reads and which versions it accepts.

use semver::Version;

/// Release channel, from the most to the least stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Channel {
    Stable,
    Beta,
    Dev,
}

/// Channel names as stored in the configuration.
pub const CHANNELS: [&str; 3] = ["stable", "beta", "dev"];

const REPO: &str = env!("CARGO_PKG_REPOSITORY");

impl Channel {
    /// Parses a configuration value; unknown values are Stable.
    pub fn parse(s: &str) -> Channel {
        match s {
            "beta" => Channel::Beta,
            "dev" => Channel::Dev,
            _ => Channel::Stable,
        }
    }

    /// The channel a version was published on: no pre-release is Stable, `dev.*` is Dev, any other is Beta.
    pub fn of(v: &Version) -> Channel {
        if v.pre.is_empty() {
            Channel::Stable
        } else if v.pre.as_str().starts_with("dev") {
            Channel::Dev
        } else {
            Channel::Beta
        }
    }

    /// Manifests this channel reads: its own and those of the more stable channels.
    pub fn endpoints(self) -> Vec<String> {
        let mut out = vec![format!("{REPO}/releases/latest/download/latest.json")];
        if self >= Channel::Beta {
            out.push(format!("{REPO}/releases/download/channels/beta.json"));
        }
        if self >= Channel::Dev {
            out.push(format!("{REPO}/releases/download/channels/dev.json"));
        }
        out
    }

    /// Whether `remote` should be offered to a build running `current` on this channel:
    /// a newer version, or any other version when the build is less stable than the channel.
    pub fn accepts(self, current: &Version, remote: &Version) -> bool {
        remote > current || (Channel::of(current) > self && remote != current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn version_channels() {
        assert_eq!(Channel::of(&v("0.2.0")), Channel::Stable);
        assert_eq!(Channel::of(&v("0.3.0-beta.1")), Channel::Beta);
        assert_eq!(Channel::of(&v("0.3.0-rc.2")), Channel::Beta);
        assert_eq!(Channel::of(&v("0.3.0-dev.41")), Channel::Dev);
    }

    #[test]
    fn newer_is_accepted_older_is_not() {
        assert!(Channel::Stable.accepts(&v("0.2.0"), &v("0.2.1")));
        assert!(!Channel::Stable.accepts(&v("0.2.1"), &v("0.2.0")));
        assert!(!Channel::Stable.accepts(&v("0.2.0"), &v("0.2.0")));
        assert!(Channel::Beta.accepts(&v("0.3.0-beta.1"), &v("0.3.0")));
    }

    #[test]
    fn switching_back_offers_a_downgrade() {
        assert!(Channel::Stable.accepts(&v("0.3.0-beta.2"), &v("0.2.0")));
        assert!(Channel::Beta.accepts(&v("0.3.0-dev.7"), &v("0.3.0-beta.1")));
        assert!(!Channel::Dev.accepts(&v("0.3.0-dev.7"), &v("0.3.0-beta.1")));
        assert!(!Channel::Beta.accepts(&v("0.3.0-beta.2"), &v("0.2.0")));
    }

    #[test]
    fn endpoints_include_more_stable_channels() {
        assert_eq!(Channel::Stable.endpoints().len(), 1);
        assert_eq!(Channel::Beta.endpoints().len(), 2);
        assert_eq!(Channel::Dev.endpoints().len(), 3);
    }
}
