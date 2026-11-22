use super::{Error, Hash, Install, Jar, Library, MAIN_CLASS, trusted_url};
use crate::download;
use serde::Deserialize;
use std::{collections::BTreeSet, path::Path};

const META: &str = "meta.fabricmc.net";
const MAVEN: &str = "maven.fabricmc.net";
const LOADER_GROUP: &str = "net.fabricmc:fabric-loader:";

#[derive(Deserialize)]
struct LoaderEntry {
    loader: Loader,
}

#[derive(Deserialize)]
struct Loader {
    version: String,
    stable: bool,
}

#[derive(Deserialize)]
pub(super) struct Profile {
    id: String,
    #[serde(rename = "inheritsFrom")]
    inherits: String,
    #[serde(rename = "mainClass")]
    main_class: String,
    pub(super) arguments: Arguments,
    libraries: Vec<MavenLibrary>,
}

#[derive(Deserialize)]
pub(super) struct Arguments {
    pub(super) jvm: Vec<String>,
    game: Vec<String>,
}

#[derive(Deserialize)]
struct MavenLibrary {
    name: String,
    url: String,
    sha256: Option<String>,
    size: Option<u64>,
}

pub(super) async fn stable_versions(install: &Install<'_>) -> Result<Vec<String>, Error> {
    let url = trusted_url(
        &format!("https://{META}/v2/versions/loader/{}", install.game),
        &[META],
    )?;
    let entries: Vec<LoaderEntry> = install.json(url, 4 * 1024 * 1024).await?;
    Ok(entries
        .into_iter()
        .filter(|item| item.loader.stable && valid_loader(&item.loader.version))
        .map(|item| item.loader.version)
        .collect())
}

pub(super) async fn profile(install: &Install<'_>, loader: &str) -> Result<Profile, Error> {
    if !valid_loader(loader) {
        return Err(Error::Invalid("Fabric Loader version"));
    }
    let url = trusted_url(
        &format!(
            "https://{META}/v2/versions/loader/{}/{loader}/profile/json",
            install.game
        ),
        &[META],
    )?;
    let profile: Profile = install.json(url, 256 * 1024).await?;
    if profile.inherits != install.game
        || profile.main_class != MAIN_CLASS
        || profile.id != format!("fabric-loader-{loader}-{}", install.game)
        || !profile.arguments.game.is_empty()
    {
        return Err(Error::Invalid("Fabric launcher profile"));
    }
    validate_profile(
        loader,
        install.game,
        &profile.arguments.jvm,
        profile.libraries.iter().map(|lib| lib.name.as_str()),
    )?;
    Ok(profile)
}

fn valid_loader(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 32
        && version.split('.').count() == 3
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

pub(super) fn validate_profile<'a>(
    loader: &str,
    game: &str,
    jvm: &[String],
    coordinates: impl Iterator<Item = &'a str>,
) -> Result<(), Error> {
    if !valid_loader(loader)
        || jvm.len() > 8
        || jvm.iter().any(|arg| {
            !matches!(
                arg.as_str(),
                "-XX:+UnlockDiagnosticVMOptions"
                    | "-XX:-UseBASE64Intrinsics"
                    | "-DFabricMcEmu= net.minecraft.client.main.Main "
            )
        })
    {
        return Err(Error::Invalid("unsupported Fabric Loader arguments"));
    }
    let required = format!("{LOADER_GROUP}{loader}");
    let intermediary = format!("net.fabricmc:intermediary:{game}");
    let mut groups = BTreeSet::new();
    let mut has_loader = false;
    for coordinate in coordinates {
        maven_path(coordinate)?;
        let (group, _) = coordinate.rsplit_once(':').unwrap();
        if groups.len() >= 32
            || !groups.insert(group)
            || (coordinate.starts_with(LOADER_GROUP) && coordinate != required)
            || (coordinate.starts_with("net.fabricmc:intermediary:") && coordinate != intermediary)
        {
            return Err(Error::Invalid("conflicting Fabric Loader libraries"));
        }
        has_loader |= coordinate == required;
    }
    if !has_loader {
        return Err(Error::Invalid("Fabric Loader library is missing"));
    }
    Ok(())
}

fn maven_path(coordinate: &str) -> Result<String, Error> {
    let mut atoms = coordinate.split(':');
    let invalid = || Error::Invalid("Fabric Maven coordinate");
    let group = atoms.next().ok_or_else(invalid)?;
    let artifact = atoms.next().ok_or_else(invalid)?;
    let version = atoms.next().ok_or_else(invalid)?;
    let valid = |value: &str| {
        !value.is_empty()
            && value.len() <= 120
            && value.as_bytes()[0].is_ascii_alphanumeric()
            && !value.ends_with('.')
            && !value.contains("..")
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+')
            })
    };
    if atoms.next().is_some() || !group.split('.').all(valid) || !valid(artifact) || !valid(version)
    {
        return Err(invalid());
    }
    Ok(format!(
        "{}/{artifact}/{version}/{artifact}-{version}.jar",
        group.replace('.', "/")
    ))
}

pub(super) async fn install(
    install: &Install<'_>,
    profile: &Profile,
    directory: &Path,
) -> Result<Vec<Library>, Error> {
    let mut downloads = Vec::new();
    let mut libraries = Vec::new();
    let mut published = BTreeSet::new();
    for item in &profile.libraries {
        if item.url != "https://maven.fabricmc.net/" {
            return Err(Error::Invalid("Fabric Maven repository"));
        }
        let url = trusted_url(
            &format!("{}{path}", item.url, path = maven_path(&item.name)?),
            &[MAVEN],
        )?;
        let hash = match &item.sha256 {
            Some(hash) => hash.to_ascii_lowercase(),
            None => {
                let checksum_url = trusted_url(&format!("{url}.sha256"), &[MAVEN])?;
                let bytes = install
                    .cancel
                    .wait(install.https.metadata(checksum_url, 256))
                    .await?
                    .map_err(Error::Https)?;
                str::from_utf8(&bytes)
                    .map_err(|_| Error::Invalid("Fabric Maven digest"))?
                    .trim()
                    .to_ascii_lowercase()
            }
        };
        let size = match item.size {
            Some(size) => size,
            None => install
                .cancel
                .wait(install.https.size(url.clone()))
                .await?
                .map_err(Error::Https)?,
        };
        let file = Jar {
            size,
            digest: Hash::Sha256(hash),
        };
        if !published.insert(file.digest.hex().to_owned()) {
            return Err(Error::Invalid("duplicate Fabric library digest"));
        }
        downloads.push(file.download(
            install.https,
            item.name.clone(),
            url,
            directory,
            install.repair,
        )?);
        libraries.push(Library {
            coordinate: item.name.clone(),
            file,
        });
    }
    download::run_verified(&downloads, None, install.cancel, install.report)
        .await
        .map_err(Error::Download)?;
    Ok(libraries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maven_paths_and_launch_entry_are_constrained() {
        assert_eq!(
            maven_path("net.fabricmc:sponge-mixin:0.17.5+mixin.0.8.7").unwrap(),
            "net/fabricmc/sponge-mixin/0.17.5+mixin.0.8.7/sponge-mixin-0.17.5+mixin.0.8.7.jar"
        );
        for path in ["a:b:../c", "a..b:c:1", "a:b:2?x", "a:b:1:classifier"] {
            assert!(maven_path(path).is_err());
        }
        assert!(
            validate_profile(
                "0.20.1",
                "1.20",
                &[],
                ["net.fabricmc:fabric-loader:0.20.1"].into_iter()
            )
            .is_ok()
        );
        assert!(
            validate_profile(
                "0.20.1",
                "1.20",
                &["-Dfabric.modsFolder=another".into()],
                ["net.fabricmc:fabric-loader:0.20.1"].into_iter()
            )
            .is_err()
        );
        assert!(
            validate_profile(
                "0.20.1",
                "1.20",
                &[],
                ["net.fabricmc:fabric-loader:0.19.0"].into_iter()
            )
            .is_err()
        );
    }
}
