use super::{Artifact, ArtifactJson, Error, Https, natives::NativeArchive, safe_path};
use crate::{
    download::{Cancellation, Download},
    game_directory::checked_subdirectory,
};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

const LIBRARY_HOSTS: &[&str] = &["libraries.minecraft.net"];

#[derive(Clone, Copy)]
pub(super) struct Platform {
    pub os: &'static str,
}

impl Platform {
    pub fn current() -> Result<Self, Error> {
        let os = std::env::consts::OS;
        if matches!(os, "linux" | "windows") && std::env::consts::ARCH == "x86_64" {
            Ok(Self { os })
        } else {
            Err(Error::Invalid("unsupported installation platform"))
        }
    }

    fn native_matches(self, classifier: &str) -> Result<bool, Error> {
        let Some(variant) = classifier.strip_prefix("natives-") else {
            return Err(Error::Invalid("native classifier"));
        };
        let (os, arch) = variant.split_once('-').unwrap_or((variant, ""));
        if os != self.os {
            return Ok(false);
        }
        match arch {
            "" | "64" | "x86_64" | "x64" | "amd64" => Ok(true),
            "32" | "x86" | "i386" | "arm64" | "aarch64" | "arm32" | "arm" => Ok(false),
            _ => Err(Error::Invalid("unsupported native architecture")),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct Library {
    name: String,
    #[serde(default)]
    downloads: Downloads,
    rules: Option<Vec<Rule>>,
    natives: Option<BTreeMap<String, String>>,
    extract: Option<Extract>,
}

#[derive(Default, Deserialize)]
struct Downloads {
    artifact: Option<LibraryFile>,
    #[serde(default)]
    classifiers: BTreeMap<String, LibraryFile>,
}

#[derive(Deserialize)]
struct LibraryFile {
    path: String,
    #[serde(flatten)]
    artifact: ArtifactJson,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    action: Action,
    os: Option<OsRule>,
    #[serde(default)]
    features: BTreeMap<String, bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Action {
    Allow,
    Disallow,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OsRule {
    name: Option<String>,
    arch: Option<String>,
    version: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Extract {
    #[serde(default)]
    exclude: Vec<String>,
}

fn allowed(rules: Option<&[Rule]>, platform: Platform) -> Result<bool, Error> {
    let Some(rules) = rules else {
        return Ok(true);
    };
    let mut allowed = false;
    for rule in rules {
        // This phase installs the base game with no optional launch features enabled.
        if rule.features.values().any(|&enabled| enabled) {
            continue;
        }
        if let Some(os) = &rule.os {
            if os.name.as_deref().is_some_and(|name| name != platform.os) {
                continue;
            }
            match os.arch.as_deref() {
                None | Some("x86_64" | "amd64" | "x64") => {}
                Some("x86" | "i386" | "i686" | "arm" | "arm32" | "arm64" | "aarch64") => continue,
                Some(_) => return Err(Error::Invalid("unsupported library architecture rule")),
            }
            if os.version.is_some() {
                return Err(Error::Invalid("unsupported library OS-version rule"));
            }
        }
        allowed = matches!(rule.action, Action::Allow);
    }
    Ok(allowed)
}

pub(super) struct Libraries {
    pub downloads: Vec<Download>,
    pub natives: Vec<NativeArchive>,
}

#[derive(PartialEq, Eq)]
struct LibraryIdentity {
    path: String,
    sha1: [u8; 20],
    size: u64,
    native: Option<Vec<String>>,
}

pub(super) fn resolve(
    libraries: Vec<Library>,
    platform: Platform,
    root: &Path,
    https: &Https,
    cancel: &Cancellation,
) -> Result<Libraries, Error> {
    let mut result = Libraries {
        downloads: Vec::new(),
        natives: Vec::new(),
    };
    let mut seen = BTreeMap::new();
    for library in libraries {
        cancel.check()?;
        if !allowed(library.rules.as_deref(), platform)? {
            continue;
        }
        let classifier = match library.name.split(':').collect::<Vec<_>>().as_slice() {
            [group, name, version]
                if [group, name, version].iter().all(|part| !part.is_empty()) =>
            {
                None
            }
            [group, name, version, classifier]
                if [group, name, version, classifier]
                    .iter()
                    .all(|part| !part.is_empty()) =>
            {
                Some(*classifier)
            }
            _ => return Err(Error::Invalid("library coordinates")),
        };
        let declared_native = classifier.filter(|classifier| classifier.starts_with("natives-"));
        if let Some(classifier) = declared_native
            && !platform.native_matches(classifier)?
        {
            continue;
        }

        let mut downloads = library.downloads;
        let excludes = library
            .extract
            .map_or_else(Vec::new, |extract| extract.exclude);
        for prefix in &excludes {
            safe_path(prefix.strip_suffix('/').unwrap_or(prefix))?;
        }
        let legacy = library
            .natives
            .as_ref()
            .and_then(|natives| natives.get(platform.os));
        if downloads.artifact.is_none() && legacy.is_none() {
            return Err(Error::Invalid("required library artifact is missing"));
        }
        if let Some(artifact) = downloads.artifact.take() {
            add(
                artifact,
                &library.name,
                declared_native.is_some().then_some(excludes.as_slice()),
                root,
                https,
                &mut seen,
                &mut result,
            )?;
        }
        if let Some(classifier) = legacy {
            let classifier = classifier.replace("${arch}", "64");
            if !platform.native_matches(&classifier)? {
                return Err(Error::Invalid("incompatible library native classifier"));
            }
            let artifact = downloads
                .classifiers
                .remove(&classifier)
                .ok_or(Error::Invalid(
                    "required library native classifier is missing",
                ))?;
            add(
                artifact,
                &library.name,
                Some(&excludes),
                root,
                https,
                &mut seen,
                &mut result,
            )?;
        }
    }
    Ok(result)
}

fn add(
    file: LibraryFile,
    name: &str,
    native: Option<&[String]>,
    root: &Path,
    https: &Https,
    seen: &mut BTreeMap<String, LibraryIdentity>,
    result: &mut Libraries,
) -> Result<(), Error> {
    let path = safe_path(&file.path)?;
    if path.extension().and_then(|suffix| suffix.to_str()) != Some("jar")
        || path.components().count() < 2
    {
        return Err(Error::Invalid("library JAR path"));
    }
    let artifact = Artifact::validate(file.artifact, LIBRARY_HOSTS)?;
    let identity = LibraryIdentity {
        path: file.path.clone(),
        sha1: artifact.sha1,
        size: artifact.size,
        native: native.map(<[String]>::to_vec),
    };
    let key = file.path.to_ascii_lowercase();
    if let Some(previous) = seen.get(&key) {
        return if *previous == identity {
            Ok(())
        } else {
            Err(Error::Invalid("conflicting library destinations"))
        };
    }
    seen.insert(key, identity);
    let parent = path.parent().ok_or(Error::Invalid("library JAR path"))?;
    let parent = checked_subdirectory(root, parent).map_err(Error::directory)?;
    let destination = parent.join(path.file_name().ok_or(Error::Invalid("library JAR path"))?);
    if let Some(excludes) = native {
        result.natives.push(NativeArchive {
            name: format!("{name} natives").into(),
            path: destination.clone(),
            sha1: artifact.sha1,
            size: artifact.size,
            excludes: excludes.to_vec(),
        });
    }
    let label = if native.is_some() {
        format!("{name} natives")
    } else {
        name.to_owned()
    };
    result
        .downloads
        .push(artifact.download(label, destination, https));
    Ok(())
}

#[cfg(test)]
mod tests;
