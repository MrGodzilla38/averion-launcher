//! Downloads a verified Fabric stack and records the exact files selected for Play.

mod loader;
mod mods;

use super::{Error, Https, trusted_url, verify_managed_directories};
use crate::{
    download::{self, Cancellation, Checksum, Download, ExistingFile, Reporter},
    game_directory::checked_subdirectory,
};
use futures::FutureExt;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MAX_JAR: u64 = 100 * 1024 * 1024;
pub(super) const MAIN_CLASS: &str = "net.fabricmc.loader.impl.launch.knot.KnotClient";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Fabric {
    pub(super) loader: String,
    pub(super) jvm: Vec<String>,
    pub(super) libraries: Vec<Library>,
    pub(super) mods: Vec<ManagedMod>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Library {
    coordinate: String,
    file: Jar,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ManagedMod {
    id: String,
    file: Jar,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Jar {
    size: u64,
    digest: Hash,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Hash {
    Sha256(String),
    Sha512(String),
}

impl Hash {
    fn hex(&self) -> &str {
        match self {
            Self::Sha256(value) | Self::Sha512(value) => value,
        }
    }

    fn checksum(&self) -> Result<Checksum, Error> {
        match self {
            Self::Sha256(value) => Ok(Checksum::Sha256(decode_hash(value)?)),
            Self::Sha512(value) => Ok(Checksum::Sha512(decode_hash(value)?)),
        }
    }
}

fn decode_hash<const N: usize>(value: &str) -> Result<[u8; N], Error> {
    if value.len() != N * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Invalid("Fabric artifact digest"));
    }
    let mut hash = [0; N];
    for (byte, pair) in hash.iter_mut().zip(value.as_bytes().as_chunks::<2>().0) {
        *byte = u8::from_str_radix(str::from_utf8(pair).unwrap(), 16)
            .map_err(|_| Error::Invalid("Fabric artifact digest"))?;
    }
    Ok(hash)
}

impl Jar {
    fn validate(&self) -> Result<Checksum, Error> {
        if !(1..=MAX_JAR).contains(&self.size) {
            return Err(Error::Invalid("Fabric artifact size"));
        }
        self.digest.checksum()
    }

    fn path(&self, directory: &Path) -> PathBuf {
        directory.join(format!("{}.jar", self.digest.hex()))
    }

    fn download(
        &self,
        service: &Https,
        label: String,
        url: Url,
        directory: &Path,
        repair: bool,
    ) -> Result<Download, Error> {
        let checksum = self.validate()?;
        let github = url.host_str() == Some("github.com");
        let https = service.clone();
        Ok(Download {
            name: label.into(),
            destination: self.path(directory),
            expected_size: Some(self.size),
            checksum: Some(checksum),
            existing: if repair {
                ExistingFile::ReplaceInvalid
            } else {
                ExistingFile::Preserve
            },
            open: Box::new(move || {
                let https = https.clone();
                let url = url.clone();
                async move {
                    if github {
                        https.github_release_asset(url).await
                    } else {
                        https.download(url).await
                    }
                }
                .boxed()
            }),
        })
    }
}

struct Install<'a> {
    https: &'a Https,
    game: &'a str,
    java: u16,
    cancel: &'a Cancellation,
    report: &'a Reporter,
    repair: bool,
}

impl Install<'_> {
    async fn json<T: serde::de::DeserializeOwned>(
        &self,
        url: Url,
        limit: usize,
    ) -> Result<T, Error> {
        let bytes = self
            .cancel
            .wait(self.https.metadata(url, limit))
            .await?
            .map_err(Error::Https)?;
        serde_json::from_slice(&bytes).map_err(|_| Error::Invalid("Fabric distribution JSON"))
    }

    async fn download(
        &self,
        file: &Jar,
        name: String,
        url: Url,
        root: &Path,
    ) -> Result<PathBuf, Error> {
        let download = file.download(self.https, name, url, root, self.repair)?;
        download::run_verified(
            std::slice::from_ref(&download),
            None,
            self.cancel,
            self.report,
        )
        .await
        .map_err(Error::Download)?;
        Ok(download.destination)
    }
}

pub(super) async fn install(
    https: &Https,
    game: &str,
    java: u16,
    version_dir: &Path,
    repair: bool,
    cancel: &Cancellation,
    report: &Reporter,
) -> Result<Fabric, Error> {
    let install = Install {
        https,
        game,
        java,
        cancel,
        report,
        repair,
    };
    let mods_dir =
        checked_subdirectory(version_dir, Path::new("fabric/mods")).map_err(Error::directory)?;
    let lib_dir = checked_subdirectory(version_dir, Path::new("fabric/libraries"))
        .map_err(Error::directory)?;
    let (loader, profile, managed) = mods::install(&install, &mods_dir).await?;
    let libraries = loader::install(&install, &profile, &lib_dir).await?;
    let fabric = Fabric {
        loader,
        jvm: profile.arguments.jvm,
        libraries,
        mods: managed,
    };
    fabric.validate(game)?;
    Ok(fabric)
}

impl Fabric {
    pub(super) fn validate(&self, game: &str) -> Result<(), Error> {
        loader::validate_profile(
            &self.loader,
            game,
            &self.jvm,
            self.libraries.iter().map(|lib| lib.coordinate.as_str()),
        )?;
        let mut ids = std::collections::BTreeSet::new();
        let mut digests = std::collections::BTreeSet::new();
        for item in &self.mods {
            item.file.validate()?;
            if !(item.id == "averion_client"
                || matches!(item.id.as_str(), "fabric" | "fabric-api")
                || mods::CURATED.iter().any(|option| option.mod_id == item.id))
                || !ids.insert(item.id.as_str())
                || !digests.insert(item.file.digest.hex())
            {
                return Err(Error::Invalid("managed Fabric mod list"));
            }
        }
        if !ids.contains("averion_client") || !(ids.contains("fabric") ^ ids.contains("fabric-api"))
        {
            return Err(Error::Invalid(
                "Fabric API or Averion Client is missing from the record",
            ));
        }
        for item in &self.libraries {
            item.file.validate()?;
        }
        Ok(())
    }

    pub(super) fn libraries(&self, version_dir: &Path) -> impl Iterator<Item = PathBuf> {
        let root = version_dir.join("fabric/libraries");
        self.libraries.iter().map(move |item| item.file.path(&root))
    }

    pub(super) fn mods(&self, version_dir: &Path) -> Vec<PathBuf> {
        let root = version_dir.join("fabric/mods");
        self.mods.iter().map(|item| item.file.path(&root)).collect()
    }

    pub(super) fn verify(&self, version_dir: &Path, cancel: &Cancellation) -> Result<(), Error> {
        let libraries = version_dir.join("fabric/libraries");
        let mods = version_dir.join("fabric/mods");
        let mut buffer = [0; 64 * 1024];
        for (name, jar, root) in self
            .libraries
            .iter()
            .map(|item| (&item.coordinate, &item.file, &libraries))
            .chain(self.mods.iter().map(|item| (&item.id, &item.file, &mods)))
        {
            verify_managed_directories(version_dir, Some(root))?;
            if download::verify_existing(
                &jar.path(root),
                Some(jar.size),
                Some(&jar.validate()?),
                cancel,
                &mut buffer,
            )? != Some(true)
            {
                return Err(Error::Damaged(format!("Fabric file {name}")));
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn fixture() -> Self {
        use sha2::{Digest, Sha256};
        let jar = |bytes: &[u8]| Jar {
            size: bytes.len() as u64,
            digest: Hash::Sha256(
                Sha256::digest(bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            ),
        };
        Self {
            loader: "0.20.1".into(),
            jvm: Vec::new(),
            libraries: vec![Library {
                coordinate: "net.fabricmc:fabric-loader:0.20.1".into(),
                file: jar(b"loader"),
            }],
            mods: vec![
                ManagedMod {
                    id: "fabric-api".into(),
                    file: jar(b"api"),
                },
                ManagedMod {
                    id: "averion_client".into(),
                    file: jar(b"client"),
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "downloads verified Fabric Loader and managed mods for representative game releases"]
    fn official_fabric_stack() {
        use futures::{FutureExt, future};
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let root = tempfile::tempdir().unwrap();
            let https = Https::default();
            for (game, java) in [("1.20", 17), ("1.16.5", 8), ("26.3", 25)] {
                let target = root.path().join(game);
                std::fs::create_dir(&target).unwrap();
                let (_cancel, cancellation) = download::cancellation();
                let report = |_| future::ready(()).boxed();
                let stack = install(&https, game, java, &target, false, &cancellation, &report)
                    .await
                    .unwrap_or_else(|error| panic!("Minecraft {game}: {error}"));
                stack.verify(&target, &cancellation).unwrap();
                assert!(stack.mods.iter().any(|item| item.id == "averion_client"));
                assert!(stack.mods.iter().any(|item| item.id == "lithium"
                    || item.id == "sodium"
                    || item.id == "ferritecore"));
                println!(
                    "Minecraft {game}: Loader {}, mods {:?}",
                    stack.loader,
                    stack.mods.iter().map(|item| &item.id).collect::<Vec<_>>()
                );
            }
        });
    }
}
