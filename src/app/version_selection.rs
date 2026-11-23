use std::{
    fs::{self, File},
    io::{self, ErrorKind, Read, Write},
    path::{Path, PathBuf},
};

use crate::{
    catalog::VERSIONS,
    game_directory::{GameDirectory, is_link},
};

const ID_LIMIT: u64 = 32;

fn selection_path(directory: &GameDirectory) -> PathBuf {
    directory.root().join("config/selected-version")
}

fn check_file(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_file() && !is_link(&metadata) {
        Ok(())
    } else {
        Err(ErrorKind::InvalidInput.into())
    }
}

fn load(path: &Path) -> io::Result<usize> {
    check_file(path)?;
    let mut id = String::new();
    File::open(path)?
        .take(ID_LIMIT + 1)
        .read_to_string(&mut id)?;
    if id.len() as u64 <= ID_LIMIT
        && let Some(selected) = VERSIONS.iter().position(|version| version.id == id)
    {
        return Ok(selected);
    }
    Err(io::Error::new(
        ErrorKind::InvalidData,
        "saved version is unavailable in the launcher catalog",
    ))
}

fn stage(directory: &GameDirectory, id: &str) -> io::Result<tempfile::NamedTempFile> {
    directory.prepare_config()?;
    let mut file = tempfile::NamedTempFile::new_in(directory.root().join("config"))?;
    file.write_all(id.as_bytes())?;
    file.as_file().sync_all()?;
    Ok(file)
}

pub(super) fn restore_or_initialize(directory: &GameDirectory) -> io::Result<usize> {
    let path = selection_path(directory);
    match load(&path) {
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        result => return result,
    }

    let default = 0;
    match stage(directory, VERSIONS[default].id)?.persist_noclobber(&path) {
        Ok(_) => Ok(default),
        Err(error) if error.error.kind() == ErrorKind::AlreadyExists => load(&path),
        Err(error) => Err(error.error),
    }
}

pub(super) fn save(directory: &GameDirectory, id: &str) -> io::Result<()> {
    let path = selection_path(directory);
    let staged = stage(directory, id)?;
    match check_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    staged.persist(&path).map_err(|error| error.error)?;
    Ok(())
}
