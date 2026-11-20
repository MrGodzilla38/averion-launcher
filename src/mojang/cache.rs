use super::Error;
use crate::game_directory::is_link;
use std::{
    fs,
    future::Future,
    io::{self, Read, Write},
    path::Path,
    time::Duration,
};

pub(super) const CACHE_TAG: &[u8] = b"Averion Mojang cache v1\n";

pub(super) async fn load<T>(
    path: &Path,
    max_age: Option<Duration>,
    limit: usize,
    validate: impl Fn(&[u8]) -> Result<T, Error>,
    fetch: impl Future<Output = Result<Vec<u8>, super::http::Error>>,
) -> Result<T, Error> {
    if let Some(bytes) = read(path, max_age, limit).map_err(Error::cache)?
        && let Ok(value) = validate(&bytes)
    {
        return Ok(value);
    }

    let bytes = fetch.await.map_err(Error::Https)?;
    if bytes.len() > limit {
        return Err(Error::Https(super::http::Error::TooLarge));
    }
    let value = validate(&bytes)?;
    publish(path, &bytes).map_err(Error::cache)?;
    Ok(value)
}

fn open_owned(path: &Path) -> io::Result<Option<(fs::File, fs::Metadata)>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(file) if file.is_file() && !is_link(&file) => file,
        Ok(_) => return Err(io::ErrorKind::InvalidInput.into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut file = fs::File::open(path)?;
    let mut tag = [0; CACHE_TAG.len()];
    match file.read_exact(&mut tag) {
        Ok(()) if tag == CACHE_TAG => Ok(Some((file, metadata))),
        Ok(()) => Err(io::ErrorKind::AlreadyExists.into()),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
            Err(io::ErrorKind::AlreadyExists.into())
        }
        Err(error) => Err(error),
    }
}

pub(super) fn read(
    path: &Path,
    max_age: Option<Duration>,
    limit: usize,
) -> io::Result<Option<Vec<u8>>> {
    let Some((file, info)) = open_owned(path)? else {
        return Ok(None);
    };
    if info.len() > (limit + CACHE_TAG.len()) as u64
        || max_age.is_some_and(|limit| {
            !info
                .modified()
                .is_ok_and(|time| time.elapsed().is_ok_and(|age| age < limit))
        })
    {
        return Ok(None);
    }

    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    Ok((bytes.len() <= limit).then_some(bytes))
}

fn publish(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let replace = open_owned(path)?.is_some();
    let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let mut stage = tempfile::Builder::new()
        .prefix(".averion-")
        .suffix(".part")
        .tempfile_in(parent)?;
    stage.write_all(CACHE_TAG)?;
    stage.write_all(bytes)?;
    stage.as_file().sync_all()?;
    if replace {
        stage.persist(path)
    } else {
        stage.persist_noclobber(path)
    }
    .map_err(|error| error.error)?;
    Ok(())
}
