//! The `arguments` section of verified Mojang version JSON, plus the rule
//! primitives shared with library rules.
//!
//! This module only models and bounds the metadata. Evaluating rules against
//! the current platform and substituting placeholders happens in
//! [`crate::launch::arguments`], which reports unsupported structures instead
//! of ignoring them.

use super::{Error, Platform};
use serde::{Deserialize, de::IgnoredAny};
use std::collections::BTreeMap;

const MAX_SECTIONS: usize = 1024;
const MAX_VALUES: usize = 64;
const MAX_RULES: usize = 64;
const MAX_FEATURES: usize = 16;
const MAX_TEMPLATE: usize = 8192;

/// Allow or disallow a rule target.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Action {
    Allow,
    Disallow,
}

/// Operating-system condition shared by library and argument rules.
#[derive(Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
    pub version: Option<String>,
}

/// A single allow/disallow decision. The last matching rule in metadata
/// order decides the outcome; rules that do not match are skipped.
#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rule {
    pub action: Action,
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: BTreeMap<String, bool>,
}

/// Selects the last matching rule. Features absent from a launch are false.
pub(crate) fn allowed(
    rules: &[Rule],
    platform: Platform,
    features: &[&str],
) -> Result<bool, Error> {
    let mut allowed = false;
    for rule in rules {
        if rule
            .features
            .iter()
            .any(|(name, expected)| features.contains(&name.as_str()) != *expected)
        {
            continue;
        }
        if let Some(os) = &rule.os {
            if os.name.as_deref().is_some_and(|name| name != platform.os) {
                continue;
            }
            match os.arch.as_deref() {
                None | Some("x86_64" | "amd64" | "x64") => {}
                Some("x86" | "i386" | "i686" | "arm" | "arm32" | "arm64" | "aarch64") => continue,
                Some(_) => return Err(Error::Invalid("unsupported architecture rule")),
            }
            if let Some(pattern) = &os.version
                && !matches_os_version(pattern, &os_version(platform)?)?
            {
                continue;
            }
        }
        allowed = rule.action == Action::Allow;
    }
    Ok(allowed)
}

// Stable Mojang metadata uses an anchored numeric major-version prefix.
// Reject unfamiliar patterns explicitly so they cannot silently change a launch.
fn matches_os_version(pattern: &str, version: &str) -> Result<bool, Error> {
    let major = pattern
        .strip_prefix('^')
        .and_then(|prefix| prefix.strip_suffix(r"\."))
        .filter(|major| !major.is_empty() && major.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or(Error::Invalid("unsupported OS-version rule"))?;
    Ok(version
        .split_once('.')
        .is_some_and(|(actual, _)| actual == major))
}

fn os_version(platform: Platform) -> Result<String, Error> {
    #[cfg(target_os = "linux")]
    if platform.os == "linux" {
        return std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map_err(|_| Error::Invalid("could not determine Linux version"));
    }
    #[cfg(windows)]
    if platform.os == "windows" {
        use windows_sys::{
            Wdk::System::SystemServices::RtlGetVersion,
            Win32::System::SystemInformation::OSVERSIONINFOW,
        };
        let mut version = OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        // RtlGetVersion fills this correctly sized, initialized structure and
        // reports the real NT version independently of compatibility manifests.
        if unsafe { RtlGetVersion(&mut version) } == 0 {
            return Ok(format!(
                "{}.{}",
                version.dwMajorVersion, version.dwMinorVersion
            ));
        }
    }
    Err(Error::Invalid("could not determine OS version"))
}

/// The modern `arguments` object. Either section may be absent; when both the
/// `arguments` object and `minecraftArguments` are absent the version cannot
/// be launched and [`Arguments::validate`] reports it.
#[derive(Deserialize, Default, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct Arguments {
    #[serde(default)]
    pub jvm: Option<Vec<Argument>>,
    #[serde(default)]
    pub game: Option<Vec<Argument>>,
    // Mojang's user-profile defaults include optional heap and GC settings.
    // Only the game and required JVM arguments drive an Averion launch.
    #[serde(rename = "default-user-jvm")]
    _default_user_jvm: Option<IgnoredAny>,
}

/// One JVM or game argument entry: a plain template, or templates guarded by
/// OS and feature rules.
#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub(crate) enum Argument {
    Plain(String),
    Conditional(Conditional),
}

/// A rule-guarded argument whose value is one string or a list of strings.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct Conditional {
    pub rules: Vec<Rule>,
    pub value: Values,
}

/// Single-value or multi-value conditional argument payload.
#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub(crate) enum Values {
    One(String),
    Many(Vec<String>),
}

impl Values {
    pub(crate) fn parts(&self) -> &[String] {
        match self {
            Self::One(value) => std::slice::from_ref(value),
            Self::Many(values) => values,
        }
    }
}

impl Arguments {
    /// Rejects empty or oversized argument structures while the verified
    /// metadata is first parsed, so launch never sees malformed input.
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        for section in [&self.jvm, &self.game] {
            let Some(arguments) = section else {
                continue;
            };
            if arguments.is_empty() || arguments.len() > MAX_SECTIONS {
                return Err("empty or oversized argument section");
            }
            for argument in arguments {
                match argument {
                    Argument::Plain(template) => validate_template(template, 1)?,
                    Argument::Conditional(conditional) => {
                        if conditional.rules.is_empty() || conditional.rules.len() > MAX_RULES {
                            return Err("empty or oversized argument rules");
                        }
                        let parts = conditional.value.parts();
                        if parts.is_empty() || parts.len() > MAX_VALUES {
                            return Err("empty or oversized argument values");
                        }
                        for rule in &conditional.rules {
                            if rule.features.len() > MAX_FEATURES {
                                return Err("too many argument rule features");
                            }
                            if let Some(os) = &rule.os
                                && [
                                    os.name.as_deref(),
                                    os.arch.as_deref(),
                                    os.version.as_deref(),
                                ]
                                .into_iter()
                                .flatten()
                                .any(|value| value.is_empty() || value.len() > 256)
                            {
                                return Err("invalid operating-system rule");
                            }
                        }
                        for part in parts {
                            validate_template(part, 0)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// True when the modern `arguments` object supplies game arguments.
    pub(crate) fn has_game(&self) -> bool {
        self.game.as_ref().is_some_and(|game| !game.is_empty())
    }
}

fn validate_template(template: &str, min: usize) -> Result<(), &'static str> {
    if template.len() < min || template.len() > MAX_TEMPLATE || template.contains('\0') {
        return Err("invalid argument template length");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<Arguments, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn parses_plain_and_conditional_entries_with_single_or_list_values() {
        let arguments = parse(
            r#"{
                "jvm": ["-cp", "${classpath}", {"rules": [{"action": "allow", "os": {"name": "windows"}}], "value": "-Xss1M"}],
                "game": [{"rules": [{"action": "allow", "features": {"has_custom_resolution": true}}], "value": ["--width", "${resolution_width}"]}]
            }"#,
        )
        .unwrap();
        arguments.validate().unwrap();
        assert!(arguments.has_game());
        let jvm = arguments.jvm.unwrap();
        assert!(matches!(&jvm[0], Argument::Plain(template) if template == "-cp"));
        let Argument::Conditional(conditional) = &jvm[2] else {
            panic!("expected a conditional JVM argument");
        };
        assert_eq!(
            conditional.rules[0].os.as_ref().unwrap().name.as_deref(),
            Some("windows")
        );
        assert_eq!(conditional.value.parts(), ["-Xss1M"]);
        let Argument::Conditional(game) = &arguments.game.unwrap()[0] else {
            panic!("expected a conditional game argument");
        };
        assert_eq!(game.value.parts(), ["--width", "${resolution_width}"]);
    }

    #[test]
    fn rejects_unknown_or_malformed_argument_structures() {
        for json in [
            r#"{"game": {"value": "no rules"}}"#,
            r#"{"game": [{"rules": [], "value": "--flag"}]}"#,
            r#"{"game": [{"rules": [{"action": "maybe", "os": {}}], "value": "--flag"}]}"#,
            r#"{"game": [{"rules": [{"action": "allow", "regex": "x"}], "value": "--flag"}]}"#,
            r#"{"game": [{"os": {"name": "linux"}, "value": "--flag"}]}"#,
            r#"{"game": [{"rules": [{"action": "allow"}], "value": []}]}"#,
            r#"{"game": [], "jvm": []}"#,
            r#"{"game": ["--ok"], "unsupported_section": []}"#,
        ] {
            assert!(
                !matches!(parse(json), Ok(arguments) if arguments.validate().is_ok()),
                "{json}"
            );
        }
        assert!(parse(r#"{"unknown": 1}"#).is_err());
    }

    #[test]
    fn matches_mojang_windows_version_gate() {
        assert!(matches_os_version(r"^10\.", "10.0").unwrap());
        assert!(!matches_os_version(r"^10\.", "6.3").unwrap());
        assert!(matches_os_version("10.*", "10.0").is_err());
    }

    #[test]
    fn bounds_template_and_section_sizes() {
        let long = "x".repeat(MAX_TEMPLATE + 1);
        let arguments = parse(&format!(r#"{{"game": ["{long}"]}}"#)).unwrap();
        assert!(arguments.validate().is_err());
        assert!(Arguments::default().validate().is_ok());
    }
}
