//! Selects release assets with published checksums and inspects their Fabric metadata.

mod metadata;

use super::{Error, Hash, Install, Jar, ManagedMod, loader, trusted_url};
use metadata::{Mod, ModSet, Versions, matches};
use reqwest::Url;
use serde::Deserialize;
use std::path::Path;

const API: &str = "api.modrinth.com";
const CDN: &str = "cdn.modrinth.com";
const FABRIC_API: &str = "P7dR8mSH";
const RELEASES: &str = "https://api.github.com/repos/MrGodzilla38/averion-client/releases";
const ASSETS: &str = "https://github.com/MrGodzilla38/averion-client/releases/download";
const MAX_API_CANDIDATES: usize = 64;

pub(super) struct Curated {
    project: &'static str,
    pub(super) mod_id: &'static str,
    name: &'static str,
}

pub(super) const CURATED: &[Curated] = &[
    Curated {
        project: "AANobbMI",
        mod_id: "sodium",
        name: "Sodium",
    },
    Curated {
        project: "gvQqBUqZ",
        mod_id: "lithium",
        name: "Lithium",
    },
    Curated {
        project: "uXXizFIs",
        mod_id: "ferritecore",
        name: "FerriteCore",
    },
];

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    published_at: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct Version {
    id: String,
    project_id: String,
    date_published: String,
    version_type: String,
    game_versions: Vec<String>,
    loaders: Vec<String>,
    dependencies: Vec<Dependency>,
    files: Vec<ModrinthFile>,
}

#[derive(Deserialize)]
struct Dependency {
    dependency_type: String,
    project_id: Option<String>,
    version_id: Option<String>,
}

#[derive(Deserialize)]
struct ModrinthFile {
    url: String,
    filename: String,
    size: u64,
    primary: bool,
    hashes: Hashes,
}

#[derive(Deserialize)]
struct Hashes {
    sha512: String,
}

pub(super) async fn install(
    install: &Install<'_>,
    directory: &Path,
) -> Result<(String, loader::Profile, Vec<ManagedMod>), Error> {
    let loaders = loader::stable_versions(install).await?;
    let api_candidates = versions(install, FABRIC_API).await?;
    let expected = format!("averion-client-{}.jar", install.game);
    // GitHub returns newer releases first. Sort each page by publication
    // date and look for the newest release with a usable stack for this game.
    for page in 1..=5 {
        let mut url = trusted_url(RELEASES, &["api.github.com"])?;
        url.query_pairs_mut()
            .append_pair("per_page", "10")
            .append_pair("page", &page.to_string());
        let mut releases: Vec<Release> = install.json(url, 4 * 1024 * 1024).await?;
        let last_page = releases.len() < 10;
        releases.sort_by(|a, b| b.published_at.cmp(&a.published_at));
        for release in releases.iter().filter(|release| {
            !release.draft && !release.prerelease && release.published_at.is_some()
        }) {
            let Some(asset) = release.assets.iter().find(|asset| asset.name == expected) else {
                continue;
            };
            let (file, url) = client_asset(release, asset, &expected)?;
            let path = install
                .download(
                    &file,
                    format!("Averion Client {}", release.tag_name),
                    url,
                    directory,
                )
                .await?;
            let client = metadata::read(&path)?;
            if client.root.id != "averion_client" {
                return Err(Error::Invalid("Averion Client mod identity"));
            }
            for loader in &loaders {
                let java = install.java.to_string();
                let base = Versions::from([
                    ("minecraft".to_owned(), install.game.to_owned()),
                    ("fabricloader".to_owned(), loader.clone()),
                    ("java".to_owned(), java),
                ]);
                if !client.root.requires("fabricloader", loader)
                    || !client.root.requires("minecraft", install.game)
                    || !client.root.requires("java", &base["java"])
                {
                    continue;
                }

                let Some((api, api_file, api_set)) =
                    fabric_api(install, &api_candidates, &base, &client, directory).await?
                else {
                    continue;
                };
                let profile = loader::profile(install, loader).await?;
                let mut managed = vec![
                    ManagedMod {
                        id: client.root.id.clone(),
                        file: file.clone(),
                    },
                    ManagedMod {
                        id: api_set.root.id.clone(),
                        file: api_file,
                    },
                ];
                let mut manifests = vec![client.clone(), api_set];
                let mut selected = Vec::new();
                for option in CURATED {
                    if let Some((entry, set, version)) = optimization(
                        install, option, api, &base, &manifests, &selected, directory,
                    )
                    .await?
                    {
                        managed.push(entry);
                        manifests.push(set);
                        selected.push(version);
                    }
                }
                return Ok((loader.clone(), profile, managed));
            }
        }
        if last_page {
            break;
        }
    }
    Err(Error::Fabric(
        "No published Averion Client, Fabric Loader and Fabric API combination matches this Minecraft version",
    ))
}

fn client_asset(release: &Release, asset: &Asset, expected: &str) -> Result<(Jar, Url), Error> {
    let tag = &release.tag_name;
    if asset.name != expected
        || tag.is_empty()
        || tag.len() > 64
        || !tag.as_bytes()[0].is_ascii_alphanumeric()
        || !tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(Error::Invalid("Averion Client release asset"));
    }
    let url = trusted_url(&asset.browser_download_url, &["github.com"])?;
    if url.as_str() != format!("{ASSETS}/{tag}/{expected}") {
        return Err(Error::Invalid("Averion Client release asset source"));
    }
    let digest = asset
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .ok_or(Error::Invalid("Averion Client release SHA-256"))?;
    let jar = Jar {
        size: asset.size,
        digest: Hash::Sha256(digest.to_ascii_lowercase()),
    };
    jar.validate()?;
    Ok((jar, url))
}

async fn versions(install: &Install<'_>, project: &str) -> Result<Vec<Version>, Error> {
    let mut url = trusted_url(
        &format!("https://{API}/v2/project/{project}/version"),
        &[API],
    )?;
    url.query_pairs_mut()
        .append_pair("loaders", r#"["fabric"]"#)
        .append_pair(
            "game_versions",
            &serde_json::to_string(&[install.game]).unwrap(),
        )
        .append_pair("include_changelog", "false");
    let mut versions: Vec<Version> = install.json(url, 12 * 1024 * 1024).await?;
    versions.retain(|version| {
        version.project_id == project
            && version.version_type == "release"
            && version.game_versions.iter().any(|id| id == install.game)
            && version.loaders.iter().any(|id| id == "fabric")
    });
    versions.sort_by(|a, b| b.date_published.cmp(&a.date_published));
    Ok(versions)
}

impl Version {
    fn dependencies_match(&self, fabric_api: Option<&str>) -> bool {
        self.dependencies.iter().all(|dependency| {
            let same_api = dependency.project_id.as_deref() == Some(FABRIC_API)
                && dependency
                    .version_id
                    .as_deref()
                    .is_none_or(|id| Some(id) == fabric_api)
                && fabric_api.is_some();
            match dependency.dependency_type.as_str() {
                "required" => same_api,
                "incompatible" => !same_api,
                "embedded" | "optional" => true,
                _ => false,
            }
        })
    }

    fn incompatible_with(&self, other: &Self) -> bool {
        self.dependencies.iter().any(|dependency| {
            dependency.dependency_type == "incompatible"
                && dependency
                    .project_id
                    .as_deref()
                    .is_none_or(|id| id == other.project_id)
                && dependency
                    .version_id
                    .as_deref()
                    .is_none_or(|id| id == other.id)
                && (dependency.project_id.is_some() || dependency.version_id.is_some())
        })
    }

    fn artifact(&self) -> Result<(Jar, Url), Error> {
        let mut primary = self.files.iter().filter(|file| file.primary);
        let file = primary
            .next()
            .or_else(|| self.files.first())
            .ok_or(Error::Invalid("missing Modrinth artifact"))?;
        if primary.next().is_some() || !file.filename.ends_with(".jar") {
            return Err(Error::Invalid("Modrinth primary mod JAR"));
        }
        let url = trusted_url(&file.url, &[CDN])?;
        let mut segments = url
            .path_segments()
            .ok_or(Error::Invalid("Modrinth artifact path"))?;
        if segments.next() != Some("data")
            || segments.next() != Some(&self.project_id)
            || segments.next() != Some("versions")
            || segments.next().is_none()
            || segments
                .next()
                .is_none_or(|segment| !segment.ends_with(".jar"))
            || segments.next().is_some()
        {
            return Err(Error::Invalid("Modrinth artifact path"));
        }
        let jar = Jar {
            size: file.size,
            digest: Hash::Sha512(file.hashes.sha512.to_ascii_lowercase()),
        };
        jar.validate()?;
        Ok((jar, url))
    }
}

async fn fabric_api<'a>(
    install: &Install<'_>,
    candidates: &'a [Version],
    base: &Versions,
    client: &ModSet,
    directory: &Path,
) -> Result<Option<(&'a Version, Jar, ModSet)>, Error> {
    for candidate in candidates.iter().take(MAX_API_CANDIDATES) {
        if !candidate.dependencies_match(None) {
            continue;
        }
        let (jar, url) = candidate.artifact()?;
        let path = install
            .download(&jar, "Fabric API".into(), url, directory)
            .await?;
        let api = metadata::read(&path)?;
        if matches!(api.root.id.as_str(), "fabric" | "fabric-api")
            && compatible(base, &[client, &api])
        {
            return Ok(Some((candidate, jar, api)));
        }
    }
    Ok(None)
}

async fn optimization(
    install: &Install<'_>,
    option: &Curated,
    api: &Version,
    base: &Versions,
    existing: &[ModSet],
    selected: &[Version],
    directory: &Path,
) -> Result<Option<(ManagedMod, ModSet, Version)>, Error> {
    for candidate in versions(install, option.project)
        .await?
        .into_iter()
        .take(12)
    {
        if !candidate.dependencies_match(Some(&api.id))
            || std::iter::once(api).chain(selected).any(|other| {
                candidate.incompatible_with(other) || other.incompatible_with(&candidate)
            })
        {
            continue;
        }
        let (file, url) = candidate.artifact()?;
        let path = install
            .download(&file, option.name.into(), url, directory)
            .await?;
        let mod_set = metadata::read(&path)?;
        let mut combined: Vec<_> = existing.iter().collect();
        combined.push(&mod_set);
        if mod_set.root.id == option.mod_id && compatible(base, &combined) {
            return Ok(Some((
                ManagedMod {
                    id: option.mod_id.into(),
                    file,
                },
                mod_set,
                candidate,
            )));
        }
    }
    Ok(None)
}

fn compatible(base: &Versions, sets: &[&ModSet]) -> bool {
    let mut versions = base.clone();
    for set in sets {
        for (id, version) in &set.versions {
            // Vanilla Loader can choose between compatible nested copies.
            // Keep existing providers here so a curated offer cannot replace
            // one of the module versions required by Averion Client.
            versions
                .entry(id.clone())
                .or_insert_with(|| version.clone());
        }
    }
    sets.iter().all(|set| check_mod(&set.root, &versions))
}

fn check_mod(module: &Mod, versions: &Versions) -> bool {
    module.environment != "server"
        && module.depends.iter().all(|(id, predicate)| {
            versions
                .get(id)
                .is_some_and(|version| matches(version, predicate))
        })
        && module.breaks.iter().all(|(id, predicate)| {
            versions
                .get(id)
                .is_none_or(|version| !matches(version, predicate))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_sources_require_exact_versioned_artifacts() {
        let release = Release {
            tag_name: "0.1.0".into(),
            published_at: Some("2026-11-22".into()),
            draft: false,
            prerelease: false,
            assets: vec![],
        };
        let mut asset = Asset {
            name: "averion-client-1.20.jar".into(),
            size: 100,
            digest: Some(format!("sha256:{}", "a".repeat(64))),
            browser_download_url: format!("{ASSETS}/0.1.0/averion-client-1.20.jar"),
        };
        assert!(client_asset(&release, &asset, "averion-client-1.20.jar").is_ok());
        assert!(client_asset(&release, &asset, "averion-client-1.20.1.jar").is_err());
        asset.digest = None;
        assert!(client_asset(&release, &asset, "averion-client-1.20.jar").is_err());
    }

    #[test]
    fn optional_mods_cannot_require_an_unselected_or_wrong_api_version() {
        let version: Version = serde_json::from_str(r#"{
            "id":"mod", "project_id":"project", "version_type":"release", "date_published":"",
            "game_versions":["1.20"], "loaders":["fabric"], "files":[],
            "dependencies":[{"dependency_type":"required","project_id":"P7dR8mSH","version_id":"api1"}]
        }"#).unwrap();
        assert!(version.dependencies_match(Some("api1")));
        assert!(!version.dependencies_match(Some("api2")));
        assert!(!version.dependencies_match(None));
    }
}
