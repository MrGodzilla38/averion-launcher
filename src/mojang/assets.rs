use super::{Artifact, Error, Https, hex_sha1, parse_sha1};
use crate::{
    download::{Cancellation, Download},
    game_directory::checked_subdirectory,
};
use serde::{
    Deserialize,
    de::{self, IgnoredAny, MapAccess, Visitor},
};
use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File},
    io::{BufReader, Read},
    path::Path,
};

pub(super) const INDEX_LIMIT: u64 = 16 * 1024 * 1024;
const MAX_OBJECTS: usize = 100_000;

#[derive(Deserialize)]
struct Index {
    #[serde(default, rename = "virtual")]
    virtual_assets: bool,
    #[serde(default)]
    map_to_resources: bool,
    #[serde(deserialize_with = "read_objects")]
    objects: BTreeMap<[u8; 20], u64>,
}

#[derive(Deserialize)]
struct Object {
    hash: String,
    size: u64,
}

fn read_objects<'de, D: de::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<[u8; 20], u64>, D::Error> {
    struct Objects;

    impl<'de> Visitor<'de> for Objects {
        type Value = BTreeMap<[u8; 20], u64>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a map of hashed asset objects")
        }

        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
            let mut objects = BTreeMap::new();
            let mut count = 0;
            while let Some((IgnoredAny, object)) = map.next_entry::<IgnoredAny, Object>()? {
                count += 1;
                if count > MAX_OBJECTS {
                    return Err(de::Error::custom("too many asset objects"));
                }
                let hash = parse_sha1(&object.hash).map_err(de::Error::custom)?;
                if let Some(size) = objects.insert(hash, object.size)
                    && size != object.size
                {
                    return Err(de::Error::custom("conflicting asset object sizes"));
                }
            }
            Ok(objects)
        }
    }

    deserializer.deserialize_map(Objects)
}

pub(super) fn resolve(
    index: &Path,
    root: &Path,
    https: &Https,
    cancel: &Cancellation,
    plan: &mut Vec<Download>,
) -> Result<(), Error> {
    let file = File::open(index).map_err(Error::directory)?;
    if file.metadata().map_err(Error::directory)?.len() > INDEX_LIMIT {
        return Err(Error::Invalid("asset index size limit"));
    }
    let index: Index = serde_json::from_reader(BufReader::new(file.take(INDEX_LIMIT + 1)))
        .map_err(|_| Error::Invalid("asset index objects"))?;
    if index.virtual_assets || index.map_to_resources || index.objects.is_empty() {
        return Err(Error::Invalid("unsupported or empty asset index"));
    }

    plan.reserve(index.objects.len());
    let mut previous = None;
    let mut directory = root.to_owned();
    for (sha1, size) in index.objects {
        cancel.check()?;
        let hash = hex_sha1(&sha1);
        let prefix = &hash[..2];
        if previous != Some(sha1[0]) {
            directory = checked_subdirectory(root, Path::new(prefix)).map_err(Error::directory)?;
            previous = Some(sha1[0]);
        }
        let artifact = Artifact {
            sha1,
            size,
            url: object_url(&hash)?,
        };
        plan.push(artifact.download(
            format!("Asset {}", &hash[..12]),
            directory.join(hash),
            https,
        ));
    }
    Ok(())
}

/// Re-checks that every asset object the index names exists with the recorded
/// size. Used before launch so a missing or truncated object is reported as a
/// repairable installation problem instead of a broken game.
pub(super) fn validate(index: &Path, root: &Path, cancel: &Cancellation) -> Result<(), Error> {
    let file = File::open(index).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::Damaged("asset index is missing".into())
        } else {
            Error::directory(error)
        }
    })?;
    if file.metadata().map_err(Error::directory)?.len() > INDEX_LIMIT {
        return Err(Error::Invalid("asset index size limit"));
    }
    let index: Index = serde_json::from_reader(BufReader::new(file.take(INDEX_LIMIT + 1)))
        .map_err(|_| Error::Invalid("asset index objects"))?;
    if index.virtual_assets || index.map_to_resources || index.objects.is_empty() {
        return Err(Error::Invalid("unsupported or empty asset index"));
    }
    let mut previous = None;
    let mut directory = root.to_owned();
    for (sha1, size) in index.objects {
        cancel.check()?;
        let hash = hex_sha1(&sha1);
        if previous != Some(sha1[0]) {
            directory = root.join(&hash[..2]);
            if !directory.symlink_metadata().is_ok_and(|metadata| {
                metadata.is_dir() && !crate::game_directory::is_link(&metadata)
            }) {
                return Err(Error::Damaged(format!("asset directory {}", &hash[..2])));
            }
            previous = Some(sha1[0]);
        }
        let path = directory.join(&hash);
        match fs::symlink_metadata(&path) {
            Ok(metadata)
                if metadata.is_file()
                    && !crate::game_directory::is_link(&metadata)
                    && metadata.len() == size => {}
            Ok(_) => {
                return Err(Error::Damaged(format!("asset object {}", &hash[..12])));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::Damaged(format!("asset object {}", &hash[..12])));
            }
            Err(error) => return Err(Error::directory(error)),
        }
    }
    Ok(())
}

fn object_url(hash: &str) -> Result<reqwest::Url, Error> {
    let prefix = hash.get(..2).ok_or(Error::Invalid("asset object digest"))?;
    let url = format!("https://resources.download.minecraft.net/{prefix}/{hash}");
    super::trusted_url(&url, &["resources.download.minecraft.net"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn asset_plan_deduplicates_hashes_and_uses_mojang_object_paths() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("index");
        let hash = "a9993e364706816aba3e25717850c26c9cd0d89d";
        fs::write(&path, format!(r#"{{"objects":{{"name":{{"hash":"{hash}","size":3}},"copy":{{"hash":"{hash}","size":3}}}}}}"#)).unwrap();
        let (_sender, cancel) = crate::download::cancellation();
        let mut plan = Vec::new();
        resolve(&path, base.path(), &Https::default(), &cancel, &mut plan).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(
            object_url(hash).unwrap().as_str(),
            "https://resources.download.minecraft.net/a9/a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(plan[0].destination, base.path().join("a9").join(hash));
        assert_eq!(plan[0].expected_size, Some(3));
        assert_eq!(
            plan[0].checksum,
            Some(crate::download::Checksum::Sha1(parse_sha1(hash).unwrap()))
        );
    }

    #[test]
    fn rejects_invalid_and_conflicting_asset_entries() {
        for input in [
            r#"{"objects":{"name":{"hash":"../invalid","size":1}}}"#,
            r#"{"objects":{"a":{"hash":"0000000000000000000000000000000000000000","size":1},"b":{"hash":"0000000000000000000000000000000000000000","size":2}}}"#,
        ] {
            assert!(serde_json::from_str::<Index>(input).is_err());
        }
    }
}
