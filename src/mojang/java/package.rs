use super::super::{
    Artifact, ArtifactJson, CLIENT_HOSTS, Error, Https, hex_sha1, parse_sha1, safe_path,
    trusted_url,
};
use crate::{
    download::{
        self, ArtifactStatus, Cancellation, Checksum, Download, ExistingFile, Failure, Progress,
        Reporter, Response, Stage,
    },
    game_directory::{ensure_directory, is_link},
};
use futures::{FutureExt, io::AllowStdIo};
use lzma_rust2::LzmaReader;
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::{self, BufReader},
    path::{Path, PathBuf},
};

const MAX_ENTRIES: usize = 2048;
const MAX_RUNTIME_BYTES: u64 = 512 * 1024 * 1024;
const DECODER_MEMORY_KIB: u32 = 64 * 1024;

#[derive(Deserialize)]
struct Manifest {
    files: BTreeMap<String, Entry>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Entry {
    Directory,
    File {
        executable: bool,
        downloads: Payloads,
    },
    Link {
        target: String,
    },
}

#[derive(Deserialize)]
struct Payloads {
    raw: ArtifactJson,
    lzma: Option<ArtifactJson>,
}

struct RuntimeFile {
    path: PathBuf,
    executable: bool,
    raw: Artifact,
    lzma: Option<Artifact>,
}

struct Link {
    path: PathBuf,
    relative_target: PathBuf,
}

pub(super) struct Package {
    folders: Vec<PathBuf>,
    files: Vec<RuntimeFile>,
    links: Vec<Link>,
    launcher: PathBuf,
}

impl Package {
    pub fn parse(
        bytes: &[u8],
        hash: &[u8; 20],
        size: Option<u64>,
        platform: &str,
    ) -> Result<Self, Error> {
        if size.is_some_and(|size| size != bytes.len() as u64) || Sha1::digest(bytes)[..] != *hash {
            return Err(Error::Java("runtime manifest integrity mismatch"));
        }
        let manifest: Manifest =
            serde_json::from_slice(bytes).map_err(|_| Error::Java("invalid runtime manifest"))?;
        if manifest.files.len() > MAX_ENTRIES {
            return Err(Error::Java("runtime entry limit"));
        }
        let launcher = if platform == "windows-x64" {
            "bin/java.exe"
        } else {
            "bin/java"
        };
        if !matches!(manifest.files.get(launcher), Some(Entry::File { executable: true, downloads }) if downloads.raw.size > 0)
        {
            return Err(Error::Java("missing Java executable in runtime manifest"));
        }
        let mut paths = HashSet::new();
        for name in manifest.files.keys() {
            safe_path(name).map_err(|_| Error::Java("unsafe runtime entry path"))?;
            if !paths.insert(name.to_ascii_lowercase()) {
                return Err(Error::Java("conflicting runtime entry paths"));
            }
            if let Some((parent, _)) = name.rsplit_once('/')
                && !matches!(manifest.files.get(parent), Some(Entry::Directory))
            {
                return Err(Error::Java("runtime entry has an invalid parent directory"));
            }
        }
        for (name, entry) in &manifest.files {
            if let Entry::Link { target } = entry {
                if platform != "linux" {
                    return Err(Error::Java("runtime package contains unsupported links"));
                }
                let target = link_target(name, target)?;
                if !matches!(
                    manifest.files.get(&target),
                    Some(Entry::Directory | Entry::File { .. })
                ) {
                    return Err(Error::Java(
                        "runtime link target is missing or is another link",
                    ));
                }
            }
        }

        let mut package = Self {
            folders: Vec::new(),
            files: Vec::new(),
            links: Vec::new(),
            launcher: launcher.into(),
        };
        let mut total = 0_u64;
        for (name, entry) in manifest.files {
            let path = PathBuf::from(name);
            match entry {
                Entry::Directory => package.folders.push(path),
                Entry::Link { target } => package.links.push(Link {
                    path,
                    relative_target: target.into(),
                }),
                Entry::File {
                    executable,
                    downloads,
                } => {
                    total = total
                        .checked_add(downloads.raw.size)
                        .filter(|&bytes| bytes <= MAX_RUNTIME_BYTES)
                        .ok_or(Error::Java("runtime size limit"))?;
                    let lzma = downloads
                        .lzma
                        .filter(|packed| packed.size > 0 && packed.size < downloads.raw.size)
                        .map(artifact)
                        .transpose()?;
                    package.files.push(RuntimeFile {
                        path,
                        executable,
                        raw: artifact(downloads.raw)?,
                        lzma,
                    });
                }
            }
        }
        Ok(package)
    }

    pub fn ready(&self, slot: &Path, cancel: &Cancellation) -> Result<Option<PathBuf>, Error> {
        let root = slot.join("runtime");
        if !safe_directory(&root)? {
            return Ok(None);
        }
        for folder in &self.folders {
            cancel.check()?;
            if !safe_directory(&root.join(folder))? {
                return Ok(None);
            }
        }
        let mut buffer = vec![0; 64 * 1024];
        for file in &self.files {
            cancel.check()?;
            let path = root.join(&file.path);
            let hash = Checksum::Sha1(file.raw.sha1);
            if download::verify_existing(
                &path,
                Some(file.raw.size),
                Some(&hash),
                cancel,
                &mut buffer,
            )? != Some(true)
            {
                return Ok(None);
            }
        }
        for link in &self.links {
            cancel.check()?;
            if !matching_link(&root, link)? {
                return Ok(None);
            }
        }
        verify_children(&root, &self.folders, &self.paths(), cancel)?;
        Ok(Some(root.join(&self.launcher)))
    }

    pub async fn install(
        self,
        slot: &Path,
        major: u16,
        https: &Https,
        cancel: &Cancellation,
        report: &Reporter,
    ) -> Result<PathBuf, Error> {
        let root = slot.join("runtime");
        let paths = self.paths();
        ensure_directory(&root).map_err(Error::directory)?;
        for directory in &self.folders {
            cancel.check()?;
            ensure_directory(&root.join(directory)).map_err(Error::directory)?;
        }
        // Saved compressed files can be resumed independently of decoded runtime files.
        let archive_dir = slot.join("archives");
        ensure_directory(&archive_dir).map_err(Error::directory)?;
        let mut archived = BTreeMap::new();
        let mut downloads = Vec::with_capacity(self.files.len());
        let mut extracts = Vec::new();
        let mut executables = Vec::new();
        for file in self.files {
            cancel.check()?;
            let destination = root.join(&file.path);
            let name = format!("Java {major} · {}", file.path.display());
            if file.executable {
                executables.push(destination.clone());
            }
            if let Some(packed) = file.lzma
                && !regular_file_exists(&destination)?
            {
                let archive = archive_dir.join(hex_sha1(&packed.sha1));
                match archived.entry(packed.sha1) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(packed.size);
                        downloads.push(packed.download(name.clone(), archive.clone(), https));
                    }
                    std::collections::btree_map::Entry::Occupied(entry)
                        if *entry.get() != packed.size =>
                    {
                        return Err(Error::Java("conflicting compressed runtime files"));
                    }
                    _ => {}
                }
                extracts.push(unpack_download(file.raw, name, destination, archive));
            } else {
                downloads.push(file.raw.download(name, destination, https));
            }
        }
        download::run_verified(&downloads, None, cancel, report)
            .await
            .map_err(Error::Download)?;
        let report_extract = |mut progress: Progress| {
            progress.stage = Stage::Extracting;
            report(progress)
        };
        let results = download::run(&extracts, cancel.clone(), report_extract)
            .await
            .map_err(|error| {
                if matches!(error.failure, Failure::Network(_)) {
                    Error::Java("could not decode a verified LZMA runtime file")
                } else {
                    Error::Download(error)
                }
            })?;
        if results.contains(&ArtifactStatus::Transferred) {
            return Err(Error::Java("unverified runtime output"));
        }

        for path in executables {
            cancel.check()?;
            make_executable(&path)?;
        }
        for link in self.links {
            cancel.check()?;
            if !matching_link(&root, &link)? {
                create_link(&root, &link)?;
            }
        }
        verify_children(&root, &self.folders, &paths, cancel)?;
        cancel.check()?;
        Ok(root.join(self.launcher))
    }

    fn paths(&self) -> HashSet<PathBuf> {
        self.folders
            .iter()
            .chain(self.files.iter().map(|file| &file.path))
            .chain(self.links.iter().map(|link| &link.path))
            .cloned()
            .collect()
    }
}

fn verify_children(
    root: &Path,
    folders: &[PathBuf],
    expected: &HashSet<PathBuf>,
    cancel: &Cancellation,
) -> Result<(), Error> {
    for folder in std::iter::once(Path::new("")).chain(folders.iter().map(PathBuf::as_path)) {
        cancel.check()?;
        for entry in fs::read_dir(root.join(folder)).map_err(Error::directory)? {
            cancel.check()?;
            let name = entry.map_err(Error::directory)?.file_name();
            if !expected.contains(&folder.join(&name))
                && !name
                    .to_str()
                    .is_some_and(|name| name.starts_with(".averion-") && name.ends_with(".part"))
            {
                return Err(Error::Java(
                    "an unexpected file in the managed runtime was preserved",
                ));
            }
        }
    }
    Ok(())
}

fn artifact(item: ArtifactJson) -> Result<Artifact, Error> {
    Ok(Artifact {
        sha1: parse_sha1(&item.sha1)?,
        size: item.size,
        url: trusted_url(&item.url, CLIENT_HOSTS)?,
    })
}

fn regular_file_exists(path: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !is_link(&metadata) => Ok(true),
        Ok(_) => Err(Error::Java("conflicting runtime file was preserved")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::directory(error)),
    }
}

fn safe_directory(path: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !is_link(&metadata) => Ok(true),
        Ok(_) => Err(Error::Java("unsafe runtime directory")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::directory(error)),
    }
}

fn make_executable(path: &Path) -> Result<(), Error> {
    if !regular_file_exists(path)? {
        return Err(Error::Java("runtime executable file is missing"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).map_err(Error::directory)?.permissions();
        if permissions.mode() & 0o100 == 0 {
            permissions.set_mode(permissions.mode() | 0o100);
            fs::set_permissions(path, permissions).map_err(Error::directory)?;
        }
    }
    Ok(())
}

fn unpack_download(
    raw: Artifact,
    name: String,
    destination: PathBuf,
    archive: PathBuf,
) -> Download {
    Download {
        name: name.into(),
        destination,
        expected_size: Some(raw.size),
        checksum: Some(Checksum::Sha1(raw.sha1)),
        existing: ExistingFile::Preserve,
        open: Box::new(move || {
            let archive = archive.clone();
            async move {
                let file = BufReader::new(File::open(archive)?);
                let decoder = LzmaReader::new_mem_limit(file, DECODER_MEMORY_KIB, None)?;
                Ok(Response {
                    body: Box::new(AllowStdIo::new(decoder)),
                    total: Some(raw.size),
                })
            }
            .boxed()
        }),
    }
}

fn link_target(name: &str, target: &str) -> Result<String, Error> {
    let mut parts: Vec<_> = name
        .rsplit_once('/')
        .map_or_else(Vec::new, |(parent, _)| parent.split('/').collect());
    for part in target.split('/') {
        match part {
            "." => {}
            ".." if parts.pop().is_some() => {}
            ".." => return Err(Error::Java("runtime link escapes its package")),
            name => {
                safe_path(name).map_err(|_| Error::Java("unsafe runtime link target"))?;
                parts.push(name);
            }
        }
    }
    Ok(parts.join("/"))
}

fn matching_link(root: &Path, link: &Link) -> Result<bool, Error> {
    let destination = root.join(&link.path);
    match fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if fs::read_link(&destination).map_err(Error::directory)? == link.relative_target {
                Ok(true)
            } else {
                Err(Error::Java("conflicting runtime link was preserved"))
            }
        }
        Ok(_) => Err(Error::Java("conflicting runtime link was preserved")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::directory(error)),
    }
}

#[cfg(unix)]
fn create_link(root: &Path, link: &Link) -> Result<(), Error> {
    std::os::unix::fs::symlink(&link.relative_target, root.join(&link.path))
        .map_err(Error::directory)
}

#[cfg(not(unix))]
fn create_link(_: &Path, _: &Link) -> Result<(), Error> {
    Err(Error::Java(
        "runtime links are unsupported on this platform",
    ))
}

#[cfg(test)]
mod tests;
