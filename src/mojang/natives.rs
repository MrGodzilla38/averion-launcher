use super::{Error, hex_sha1, libraries::Platform, safe_path};
use crate::{
    download::Cancellation,
    game_directory::{checked_subdirectory, is_link},
};
use sha1::{Digest, Sha1};
use std::{
    fs::{self, File},
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use zip::{CompressionMethod, ZipArchive};

const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;

pub(super) struct NativeArchive {
    pub name: Arc<str>,
    pub path: PathBuf,
    pub sha1: [u8; 20],
    pub size: u64,
    pub excludes: Vec<String>,
}

/// Extracts the verified platform binaries of one archive into its own
/// `natives/<sha1>` directory and returns the extracted paths relative to
/// that directory.
pub(super) fn extract(
    native: &NativeArchive,
    version_dir: &Path,
    platform: Platform,
    cancel: &Cancellation,
) -> Result<Vec<PathBuf>, Error> {
    let file = open_regular(&native.path)?.ok_or(Error::Native("native archive is missing"))?;
    let mut buffer = vec![0; 64 * 1024];
    let file = verify(file, native, &mut buffer, cancel)?;
    let mut archive = ZipArchive::new(file).map_err(|_| Error::Native("invalid ZIP archive"))?;
    let entries = entries(&mut archive, native, platform, cancel)?;
    let relative = Path::new("natives").join(hex_sha1(&native.sha1));
    let destination = checked_subdirectory(version_dir, &relative).map_err(Error::directory)?;
    let mut comparison = vec![0; buffer.len()];
    let mut produced = Vec::with_capacity(entries.len());
    for (index, path) in entries {
        cancel.check()?;
        let parent = checked_subdirectory(&destination, path.parent().unwrap_or(Path::new("")))
            .map_err(Error::directory)?;
        let destination = parent.join(
            path.file_name()
                .ok_or(Error::Native("invalid output name"))?,
        );
        let mut entry = archive
            .by_index(index)
            .map_err(|_| Error::Native("invalid ZIP entry"))?;
        let size = entry.size();
        if let Some(mut existing) = open_regular(&destination)? {
            if existing.metadata().map_err(Error::directory)?.len() != size {
                return Err(Error::Native("conflicting native file was preserved"));
            }
            copy_entry(&mut entry, size, &mut buffer, cancel, |chunk| {
                existing
                    .read_exact(&mut comparison[..chunk.len()])
                    .map_err(Error::directory)?;
                if comparison[..chunk.len()] != *chunk {
                    return Err(Error::Native("conflicting native file was preserved"));
                }
                Ok(())
            })?;
        } else {
            let mut stage = tempfile::Builder::new()
                .prefix(".averion-")
                .suffix(".part")
                .tempfile_in(parent)
                .map_err(Error::directory)?;
            copy_entry(&mut entry, size, &mut buffer, cancel, |chunk| {
                stage.write_all(chunk).map_err(Error::directory)
            })?;
            stage.as_file().sync_all().map_err(Error::directory)?;
            cancel.check()?;
            stage.persist_noclobber(destination).map_err(|error| {
                if error.error.kind() == io::ErrorKind::AlreadyExists {
                    Error::Native("conflicting native file was preserved")
                } else {
                    Error::directory(error.error)
                }
            })?;
        }
        produced.push(path);
    }
    cancel.check()?;
    Ok(produced)
}

/// Merges the extracted archives of one version into the single
/// `natives/launch` directory that `${natives_directory}` points at, and
/// returns the assembled files (relative to the version directory) with the
/// size and SHA-1 recorded for launch-time verification. Files that already
/// match are kept; differing launcher-owned files are replaced, while two
/// archives claiming the same name with different contents is an error.
pub(super) fn assemble(
    version_dir: &Path,
    archives: &[(String, Vec<PathBuf>)],
    cancel: &Cancellation,
) -> Result<Vec<(PathBuf, u64, [u8; 20])>, Error> {
    let launch_relative = Path::new("natives").join("launch");
    let launch = checked_subdirectory(version_dir, &launch_relative).map_err(Error::directory)?;
    let mut claimed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut outputs = Vec::new();
    for (archive, files) in archives {
        let source_root = checked_subdirectory(version_dir, &Path::new("natives").join(archive))
            .map_err(Error::directory)?;
        for file in files {
            cancel.check()?;
            let source = source_root.join(file);
            let target_relative = launch_relative.join(file);
            let target = launch.join(file);
            let key = target_relative.to_string_lossy().to_ascii_lowercase();
            if claimed.contains(&key) {
                // Two archives may ship the same binary; identical contents
                // are fine, differing contents are not.
                if !identical_contents(&source, &target, cancel)? {
                    return Err(Error::Native("conflicting native binaries"));
                }
                continue;
            }
            if let Some(parent) = file.parent() {
                checked_subdirectory(&launch, parent).map_err(Error::directory)?;
            }
            let (size, sha1) = copy_file(&source, &target, cancel)?;
            claimed.insert(key);
            outputs.push((target_relative, size, sha1));
        }
    }
    Ok(outputs)
}

fn identical_contents(left: &Path, right: &Path, cancel: &Cancellation) -> Result<bool, Error> {
    let mut left = open_regular(left)?.ok_or(Error::Native("native binary is missing"))?;
    let mut right = match open_regular(right)? {
        Some(file) => file,
        None => return Ok(false),
    };
    if left.metadata().map_err(Error::directory)?.len()
        != right.metadata().map_err(Error::directory)?.len()
    {
        return Ok(false);
    }
    let mut left_buffer = vec![0; 64 * 1024];
    let mut right_buffer = vec![0; 64 * 1024];
    loop {
        cancel.check()?;
        let left_count = left.read(&mut left_buffer).map_err(Error::directory)?;
        let right_count = right.read(&mut right_buffer).map_err(Error::directory)?;
        if left_count != right_count || left_buffer[..left_count] != right_buffer[..right_count] {
            return Ok(false);
        }
        if left_count == 0 {
            return Ok(true);
        }
    }
}

/// Copies a verified source into the launcher-owned launch directory in one
/// pass, returning the assembled size and digest so the installation record
/// can pin exactly what launch must find.
fn copy_file(
    source: &Path,
    target: &Path,
    cancel: &Cancellation,
) -> Result<(u64, [u8; 20]), Error> {
    let mut input = open_regular(source)?.ok_or(Error::Native("native binary is missing"))?;
    let size = input.metadata().map_err(Error::directory)?.len();
    if size == 0 {
        return Err(Error::Native("empty native binary"));
    }
    let parent = target
        .parent()
        .ok_or(Error::Native("invalid native path"))?;
    let mut stage = tempfile::Builder::new()
        .prefix(".averion-")
        .suffix(".part")
        .tempfile_in(parent)
        .map_err(Error::directory)?;
    let mut buffer = vec![0; 64 * 1024];
    let mut hasher = Sha1::new();
    let mut written = 0_u64;
    loop {
        cancel.check()?;
        let count = input.read(&mut buffer).map_err(Error::directory)?;
        if count == 0 {
            break;
        }
        written += count as u64;
        if written > size {
            return Err(Error::Native("native binary grew while assembling"));
        }
        stage
            .write_all(&buffer[..count])
            .map_err(Error::directory)?;
        hasher.update(&buffer[..count]);
    }
    if written != size {
        return Err(Error::Native("native binary shrank while assembling"));
    }
    stage.as_file().sync_all().map_err(Error::directory)?;
    cancel.check()?;
    let digest: [u8; 20] = hasher.finalize().into();
    stage
        .persist(target)
        .map_err(|error| Error::directory(error.error))?;
    Ok((size, digest))
}

fn open_regular(path: &Path) -> Result<Option<File>, Error> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !is_link(&meta) => {
            File::open(path).map(Some).map_err(Error::directory)
        }
        Ok(_) => Err(Error::Native("unsafe native file path")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::directory(error)),
    }
}

fn verify(
    mut file: File,
    native: &NativeArchive,
    buffer: &mut [u8],
    cancel: &Cancellation,
) -> Result<File, Error> {
    if native.size > MAX_ARCHIVE_BYTES
        || file.metadata().map_err(Error::directory)?.len() != native.size
    {
        return Err(Error::Native("native archive size mismatch"));
    }
    let mut sha1 = Sha1::new();
    copy_entry(&mut file, native.size, buffer, cancel, |bytes| {
        sha1.update(bytes);
        Ok(())
    })?;
    if sha1.finalize()[..] != native.sha1 {
        return Err(Error::Native("native archive checksum mismatch"));
    }
    file.rewind().map_err(Error::directory)?;
    Ok(file)
}

fn entries(
    archive: &mut ZipArchive<File>,
    native: &NativeArchive,
    platform: Platform,
    cancel: &Cancellation,
) -> Result<Vec<(usize, PathBuf)>, Error> {
    if archive.len() > MAX_ENTRIES
        || archive
            .has_overlapping_files()
            .map_err(|_| Error::Native("invalid ZIP structure"))?
    {
        return Err(Error::Native("unsafe ZIP structure"));
    }
    let mut entries = Vec::new();
    let mut paths = std::collections::HashSet::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        cancel.check()?;
        let entry = archive
            .by_index_data(index)
            .map_err(|_| Error::Native("invalid ZIP entry"))?;
        let name = std::str::from_utf8(entry.name_raw())
            .map_err(|_| Error::Native("invalid ZIP entry name"))?;
        let path = safe_path(name.strip_suffix('/').unwrap_or(name))
            .map_err(|_| Error::Native("unsafe ZIP entry path"))?;
        let unix_type = entry.unix_mode().unwrap_or_default() & 0o170000;
        let proper_type = if entry.is_dir() {
            matches!(unix_type, 0 | 0o040000)
        } else {
            matches!(unix_type, 0 | 0o100000)
        };
        if entry.is_symlink()
            || !proper_type
            || entry.encrypted()
            || !matches!(
                entry.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            )
        {
            return Err(Error::Native("unsafe or unsupported ZIP entry"));
        }
        let name_lower = name.to_ascii_lowercase();
        if !paths.insert(name_lower.trim_end_matches('/').to_owned()) {
            return Err(Error::Native("duplicate ZIP paths"));
        }
        if entry.is_dir()
            || name_lower.starts_with("meta-inf/")
            || native
                .excludes
                .iter()
                .any(|prefix| name.starts_with(prefix))
            || !is_binary(&name_lower, platform)
        {
            continue;
        }
        total = total
            .checked_add(entry.size())
            .filter(|&bytes| bytes <= MAX_EXTRACTED_BYTES)
            .ok_or(Error::Native("native extraction size limit"))?;
        if entry.size() == 0 {
            return Err(Error::Native("empty native binary"));
        }
        entries.push((index, path));
    }
    if entries.is_empty() {
        return Err(Error::Native("archive has no usable native binaries"));
    }
    Ok(entries)
}

fn is_binary(name: &str, platform: Platform) -> bool {
    let filename = name.rsplit('/').next().unwrap_or(name);
    match platform.os {
        "windows" => filename.ends_with(".dll"),
        "linux" => {
            filename.ends_with(".so")
                || filename.split_once(".so.").is_some_and(|(_, suffix)| {
                    suffix.split('.').all(|part| {
                        !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
                    })
                })
        }
        _ => false,
    }
}

fn copy_entry(
    reader: &mut impl Read,
    size: u64,
    buffer: &mut [u8],
    cancel: &Cancellation,
    mut write: impl FnMut(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut received = 0_u64;
    loop {
        cancel.check()?;
        let count = reader
            .read(buffer)
            .map_err(|_| Error::Native("could not read verified native bytes"))?;
        received += count as u64;
        if received > size || (count == 0 && received != size) {
            return Err(Error::Native("native file size mismatch"));
        }
        if count == 0 {
            return Ok(());
        }
        write(&buffer[..count])?;
    }
}

#[cfg(test)]
mod tests;
