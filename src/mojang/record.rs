//! Persisted per-version installation record.
//!
//! The record is written atomically after a successful install and is the
//! source of truth for readiness checks and launch-time validation. It stores
//! no secrets: only the verified metadata checksum, the Java runtime that was
//! resolved, the assembled native binaries, the logging configuration ID and
//! the verified Fabric stack.
//!
//! Two launcher-owned files live in the version directory:
//! - `<id>.json` — the verified Mojang version metadata, so launches never
//!   trust the network or an unverified cache entry;
//! - `averion.json` — this record, whose `metadata_sha1` pins the metadata.

use super::{Error, Runtime, VERSION_LIMIT, fabric::Fabric, hex_sha1, safe_path};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
};

pub(super) const SCHEMA: u32 = 2;
pub(super) const FILE: &str = "averion.json";
const RECORD_LIMIT: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Record {
    pub(crate) schema: u32,
    pub(crate) version: String,
    /// Lowercase hex SHA-1 of the version metadata written beside this record.
    pub(crate) metadata_sha1: String,
    pub(crate) runtime: Runtime,
    /// Native binaries assembled for launch, relative to the version directory,
    /// with the size and digest launch re-verifies before starting the game.
    pub(crate) natives: Vec<NativeEntry>,
    /// Id of the logging configuration file, when the version declares one.
    pub(crate) log_config: Option<String>,
    #[serde(default)]
    pub(super) fabric: Option<Fabric>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct NativeEntry {
    pub(crate) path: String,
    pub(crate) size: u64,
    pub(crate) sha1: String,
}

/// Writes the verified metadata and the record. Both files are launcher-owned:
/// the metadata file follows the layout every Minecraft launcher uses for
/// `<id>.json`, and the record only ever describes launcher-produced state.
/// The metadata digest recorded is verified here so the pair can never
/// disagree.
pub(super) fn write(
    version_dir: &Path,
    id: &str,
    metadata: &[u8],
    record: &Record,
) -> Result<(), Error> {
    let digest: [u8; 20] = Sha1::digest(metadata).into();
    if hex_sha1(&digest) != record.metadata_sha1 {
        return Err(Error::Invalid("installation record metadata digest"));
    }
    let document =
        serde_json::to_vec(record).map_err(|_| Error::Invalid("installation record encoding"))?;
    if document.len() > RECORD_LIMIT {
        return Err(Error::Invalid("installation record size"));
    }
    atomic_write(&version_dir.join(format!("{id}.json")), metadata)?;
    atomic_write(&version_dir.join(FILE), &document)
}

/// Loads the record for a version directory. `Ok(None)` means no install has
/// completed for this version; anything else is reported as damaged state that
/// a repair install rewrites.
pub(crate) fn load(version_dir: &Path) -> Result<Option<Record>, Error> {
    let bytes = match read_file(&version_dir.join(FILE), RECORD_LIMIT) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Error::record(error)),
    };
    if bytes.len() > RECORD_LIMIT {
        return Err(Error::Invalid("installation record size"));
    }
    let record: Record =
        serde_json::from_slice(&bytes).map_err(|_| Error::Invalid("installation record format"))?;
    if record.schema == 1 && record.fabric.is_none() {
        return Ok(None);
    }
    if record.schema != SCHEMA || record.version.is_empty() || record.version.len() > 64 {
        return Err(Error::Invalid("installation record schema"));
    }
    if record.metadata_sha1.len() != 40
        || !record
            .metadata_sha1
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Error::Invalid("installation record metadata digest"));
    }
    if record.runtime.major < 8 || !record.runtime.executable.is_absolute() {
        return Err(Error::Invalid("installation record Java runtime"));
    }
    if record.natives.len() > 4096 {
        return Err(Error::Invalid("installation record natives"));
    }
    for entry in &record.natives {
        let path = entry
            .path
            .strip_prefix("natives/launch/")
            .ok_or(Error::Invalid("installation record native path"))?;
        safe_path(path).map_err(|_| Error::Invalid("installation record native path"))?;
        if entry.size == 0
            || entry.sha1.len() != 40
            || !entry.sha1.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(Error::Invalid("installation record native entry"));
        }
    }
    if let Some(id) = &record.log_config
        && (id.is_empty() || id.len() > 64)
    {
        return Err(Error::Invalid("installation record logging id"));
    }
    record
        .fabric
        .as_ref()
        .ok_or(Error::Invalid("installation record is missing Fabric"))?
        .validate(&record.version)?;
    Ok(Some(record))
}

/// Reads the version metadata written at install time and verifies it against
/// the digest pinned by the record, so launch never parses unverified bytes.
pub(super) fn metadata(version_dir: &Path, id: &str, record: &Record) -> Result<Vec<u8>, Error> {
    let bytes =
        read_file(&version_dir.join(format!("{id}.json")), VERSION_LIMIT).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                Error::Invalid("version metadata is missing")
            } else {
                Error::record(error)
            }
        })?;
    if bytes.len() > VERSION_LIMIT {
        return Err(Error::Invalid("version metadata size"));
    }
    let digest: [u8; 20] = Sha1::digest(&bytes).into();
    if hex_sha1(&digest) != record.metadata_sha1 {
        return Err(Error::Invalid("version metadata checksum"));
    }
    Ok(bytes)
}

fn read_file(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || crate::game_directory::is_link(&metadata) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let parent = path.parent().ok_or(Error::Invalid("record path"))?;
    let mut stage = tempfile::Builder::new()
        .prefix(".averion-")
        .suffix(".part")
        .tempfile_in(parent)
        .map_err(Error::record)?;
    stage.write_all(bytes).map_err(Error::record)?;
    stage.as_file().sync_all().map_err(Error::record)?;
    stage
        .persist(path)
        .map_err(|error| Error::record(error.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_directory::GameDirectory;

    fn record(version: &str) -> Record {
        Record {
            schema: SCHEMA,
            version: version.to_owned(),
            metadata_sha1: "0".repeat(40),
            runtime: Runtime {
                executable: Path::new(if cfg!(windows) {
                    r"C:\Java\bin\java.exe"
                } else {
                    "/opt/java/bin/java"
                })
                .to_owned(),
                major: 21,
            },
            natives: vec![NativeEntry {
                path: "natives/launch/liblwjgl.so".to_owned(),
                size: 12,
                sha1: "a".repeat(40),
            }],
            log_config: Some("client-1.21.2.xml".to_owned()),
            fabric: Some(Fabric::fixture()),
        }
    }

    #[test]
    fn failed_update_preserves_the_last_verified_fabric_record() {
        let base = tempfile::tempdir().unwrap();
        let original = b"verified metadata";
        let mut installed = record("1.20");
        installed.metadata_sha1 = hex_sha1(&Sha1::digest(original).into());
        write(base.path(), "1.20", original, &installed).unwrap();

        let mut update = installed.clone();
        update.fabric.as_mut().unwrap().loader = "0.20.2".into();
        let error = write(base.path(), "1.20", b"incomplete update", &update).unwrap_err();
        assert!(matches!(
            error,
            Error::Invalid("installation record metadata digest")
        ));
        assert_eq!(load(base.path()).unwrap(), Some(installed));
    }

    #[test]
    fn legacy_record_stays_on_disk_but_requires_fabric_installation() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join(FILE);
        let mut legacy = record("1.20");
        legacy.schema = 1;
        legacy.fabric = None;
        let bytes = serde_json::to_vec(&legacy).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(load(base.path()).unwrap().is_none());
        assert_eq!(fs::read(&path).unwrap(), bytes);

        legacy.schema = SCHEMA;
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert!(matches!(
            load(base.path()),
            Err(Error::Invalid("installation record is missing Fabric"))
        ));
    }

    #[test]
    fn round_trips_metadata_and_record_and_reports_tampering() {
        let base = tempfile::tempdir().unwrap();
        let directory = GameDirectory::for_test(base.path());
        let version_dir = directory.version_path("1.21.4");
        fs::create_dir_all(&version_dir).unwrap();
        let document = br#"{"id":"1.21.4"}"#;
        let sha1: [u8; 20] = Sha1::digest(document).into();
        let mut expected = record("1.21.4");
        expected.metadata_sha1 = hex_sha1(&sha1);

        assert!(load(&version_dir).unwrap().is_none());
        write(&version_dir, "1.21.4", document, &expected).unwrap();

        let loaded = load(&version_dir).unwrap().unwrap();
        assert_eq!(loaded, expected);
        assert_eq!(
            metadata(&version_dir, "1.21.4", &loaded).unwrap(),
            document.to_vec()
        );

        // A modified metadata file no longer matches the pinned digest.
        fs::write(version_dir.join("1.21.4.json"), b"{\"id\":\"9.9.9\"}").unwrap();
        assert!(matches!(
            metadata(&version_dir, "1.21.4", &loaded),
            Err(Error::Invalid("version metadata checksum"))
        ));

        // A missing metadata file is reported as damaged state, not as an IO bug.
        fs::remove_file(version_dir.join("1.21.4.json")).unwrap();
        assert!(matches!(
            metadata(&version_dir, "1.21.4", &loaded),
            Err(Error::Invalid("version metadata is missing"))
        ));
    }

    #[test]
    fn rejects_records_with_unsafe_or_foreign_contents() {
        let base = tempfile::tempdir().unwrap();
        let directory = GameDirectory::for_test(base.path());
        let version_dir = directory.version_path("1.20.1");
        fs::create_dir_all(&version_dir).unwrap();

        let mut foreign = record("1.20.1");
        foreign.schema = SCHEMA + 1;
        fs::write(
            version_dir.join(FILE),
            serde_json::to_vec(&foreign).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            load(&version_dir),
            Err(Error::Invalid("installation record schema"))
        ));

        let mut escaped = record("1.20.1");
        escaped.natives = vec![NativeEntry {
            path: "../../outside.so".to_owned(),
            size: 1,
            sha1: "a".repeat(40),
        }];
        fs::write(
            version_dir.join(FILE),
            serde_json::to_vec(&escaped).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            load(&version_dir),
            Err(Error::Invalid("installation record native path"))
        ));

        fs::write(version_dir.join(FILE), b"not json").unwrap();
        assert!(matches!(
            load(&version_dir),
            Err(Error::Invalid("installation record format"))
        ));
    }

    #[test]
    fn stores_no_secrets_in_the_record() {
        let base = tempfile::tempdir().unwrap();
        let directory = GameDirectory::for_test(base.path());
        let version_dir = directory.version_path("1.20.1");
        fs::create_dir_all(&version_dir).unwrap();
        let metadata = b"{}";
        let sha1: [u8; 20] = Sha1::digest(metadata).into();
        let mut expected = record("1.20.1");
        expected.metadata_sha1 = hex_sha1(&sha1);
        write(&version_dir, "1.20.1", metadata, &expected).unwrap();
        let text = fs::read_to_string(version_dir.join(FILE))
            .unwrap()
            .to_ascii_lowercase();
        assert!(!text.contains("token"));
        assert!(!text.contains("password"));
        assert!(!text.contains("secret"));
    }

    #[test]
    fn refuses_a_record_that_disagrees_with_the_metadata() {
        let base = tempfile::tempdir().unwrap();
        let directory = GameDirectory::for_test(base.path());
        let version_dir = directory.version_path("1.20.1");
        fs::create_dir_all(&version_dir).unwrap();
        let mut mismatched = record("1.20.1");
        mismatched.metadata_sha1 = "b".repeat(40);
        assert!(matches!(
            write(&version_dir, "1.20.1", b"{}", &mismatched),
            Err(Error::Invalid("installation record metadata digest"))
        ));
    }
}
