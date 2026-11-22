use crate::mojang::{Error, safe_path};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Cursor, Read, Seek},
    path::Path,
};
use zip::ZipArchive;

const METADATA_LIMIT: u64 = 256 * 1024;
const NESTED_LIMIT: u64 = 24 * 1024 * 1024;
const MAX_NESTED: usize = 256;
const NESTED_TOTAL_LIMIT: u64 = 96 * 1024 * 1024;

pub(super) type Versions = BTreeMap<String, String>;

#[derive(Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum Predicate {
    Single(String),
    Any(Vec<String>),
}

#[derive(Clone, Deserialize)]
pub(super) struct Mod {
    pub id: String,
    version: String,
    #[serde(default)]
    pub environment: String,
    #[serde(default)]
    provides: Vec<String>,
    #[serde(default)]
    pub depends: BTreeMap<String, Predicate>,
    #[serde(default)]
    pub breaks: BTreeMap<String, Predicate>,
    #[serde(default)]
    jars: Vec<Nested>,
}

impl Mod {
    pub(super) fn requires(&self, id: &str, version: &str) -> bool {
        self.depends
            .get(id)
            .is_some_and(|predicate| matches(version, predicate))
    }
}

#[derive(Clone, Deserialize)]
struct Nested {
    file: String,
}

#[derive(Clone)]
pub(super) struct ModSet {
    pub root: Mod,
    pub versions: Versions,
}

pub(super) fn read(path: &Path) -> Result<ModSet, Error> {
    let invalid = Error::Invalid("Fabric mod archive");
    let file = File::open(path).map_err(Error::directory)?;
    let mut archive = ZipArchive::new(file).map_err(|_| invalid.clone())?;
    let root = manifest(&mut archive)?;
    let mut versions = Versions::new();
    providers(&root, &mut versions)?;
    let mut remaining = NESTED_TOTAL_LIMIT;
    for nested in &root.jars {
        safe_path(&nested.file)?;
        let child = archive.by_name(&nested.file).map_err(|_| invalid.clone())?;
        let limit = remaining.min(NESTED_LIMIT);
        if child.size() > limit {
            return Err(invalid);
        }
        let mut bytes = Vec::new();
        child
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(Error::directory)?;
        if bytes.len() as u64 > limit {
            return Err(invalid);
        }
        remaining -= bytes.len() as u64;
        let mut nested = ZipArchive::new(Cursor::new(bytes)).map_err(|_| invalid.clone())?;
        providers(&manifest(&mut nested)?, &mut versions)?;
    }
    Ok(ModSet { root, versions })
}

fn manifest<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<Mod, Error> {
    let invalid = Error::Invalid("fabric.mod.json");
    let file = archive
        .by_name("fabric.mod.json")
        .map_err(|_| invalid.clone())?;
    if file.size() > METADATA_LIMIT {
        return Err(invalid);
    }
    let mut data = Vec::new();
    file.take(METADATA_LIMIT + 1)
        .read_to_end(&mut data)
        .map_err(Error::directory)?;
    let manifest: Mod = serde_json::from_slice(&data).map_err(|_| invalid.clone())?;
    if data.len() as u64 > METADATA_LIMIT || manifest.jars.len() > MAX_NESTED {
        return Err(invalid);
    }
    Ok(manifest)
}

fn providers(module: &Mod, versions: &mut Versions) -> Result<(), Error> {
    for id in std::iter::once(&module.id).chain(&module.provides) {
        if id.is_empty()
            || id.len() > 64
            || !id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
            })
        {
            return Err(Error::Invalid("Fabric mod ID"));
        }
        if module.environment != "server" {
            versions
                .entry(id.clone())
                .or_insert_with(|| module.version.clone());
        }
    }
    Ok(())
}

pub(super) fn matches(version: &str, predicate: &Predicate) -> bool {
    match predicate {
        Predicate::Single(rule) => condition(version, rule),
        Predicate::Any(rules) => rules.iter().any(|rule| condition(version, rule)),
    }
}

fn condition(version: &str, rule: &str) -> bool {
    rule.split("||").any(|alternative| {
        let alternative = alternative.trim();
        !alternative.is_empty()
            && alternative
                .split_whitespace()
                .all(|term| satisfies(version, term))
    })
}

fn satisfies(version: &str, term: &str) -> bool {
    if term == "*" {
        return true;
    }
    let (operator, target) = [">=", "<=", ">", "<", "=", "^", "~"]
        .iter()
        .find_map(|operator| term.strip_prefix(operator).map(|value| (*operator, value)))
        .unwrap_or(("=", term));
    if operator == "=" && (version == target || target.contains('+')) {
        return version == target;
    }
    let Some(actual) = Version::parse(version) else {
        return false;
    };
    if operator == "="
        && let Some(prefix) = target
            .strip_suffix(".x")
            .or_else(|| target.strip_suffix(".*"))
            .or_else(|| target.strip_suffix(".X"))
    {
        let components = prefix.split('.').collect::<Vec<_>>();
        return components.len() < 3
            && components
                .iter()
                .zip(actual.numbers)
                .all(|(part, found)| part.parse::<u32>() == Ok(found));
    }
    let Some(required) = Version::parse(target) else {
        return false;
    };
    match operator {
        "=" => actual == required,
        ">=" => actual >= required,
        "<=" => actual <= required,
        ">" => actual > required,
        "<" => actual < required,
        "~" => actual >= required && actual.numbers[..2] == required.numbers[..2],
        "^" => {
            let boundary = required
                .numbers
                .iter()
                .position(|&value| value != 0)
                .unwrap_or(2);
            actual >= required && actual.numbers[..=boundary] == required.numbers[..=boundary]
        }
        _ => false,
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Version<'a> {
    numbers: [u32; 3],
    stable: bool,
    suffix: &'a str,
}

impl<'a> Version<'a> {
    fn parse(version: &'a str) -> Option<Self> {
        let core = version.split('+').next()?;
        let (numeric, suffix) = core.split_once('-').unwrap_or((core, ""));
        let mut numbers = [0; 3];
        for (index, part) in numeric.split('.').enumerate() {
            *numbers.get_mut(index)? = part.parse().ok()?;
        }
        Some(Self {
            numbers,
            stable: !core.contains('-'),
            suffix,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_module_pins_and_rejects_other_stable_releases() {
        for (version, rule, expected) in [
            ("0.4.29+b04edc7a27", "=0.4.29+b04edc7a27", true),
            ("0.4.29+alternate", "=0.4.29+b04edc7a27", false),
            ("1.20", ">=1.20- <1.20.1-", true),
            ("1.20.1", ">=1.20- <1.20.1-", false),
            ("1.16.5", "1.16.x", true),
            ("1.16.5", "1.17.*", false),
            ("0.20.1", ">=0.14.21", true),
            ("0.20.1", ">=0.21.0", false),
        ] {
            assert_eq!(
                matches(version, &Predicate::Single(rule.into())),
                expected,
                "{version} vs {rule}"
            );
        }
    }
}
