use super::*;
use futures::{executor::block_on, future};
use serde_json::{Value, json};

const RAW: &[u8] = b"Averion runtime fixture\n";
const LZMA: &[u8] = &[
    93, 0, 0, 16, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 32, 157, 136, 167, 85, 187, 92,
    243, 91, 180, 199, 205, 160, 28, 190, 143, 224, 82, 165, 41, 189, 170, 44, 250, 165, 121, 245,
    239, 255, 255, 148, 104, 0, 0,
];

fn sha1(bytes: &[u8]) -> [u8; 20] {
    Sha1::digest(bytes).into()
}

fn payload(bytes: &[u8]) -> Value {
    json!({"sha1": hex_sha1(&sha1(bytes)), "size": bytes.len(), "url": "https://piston-data.mojang.com/runtime-fixture"})
}

fn fixture(platform: &str) -> Value {
    let java = if platform == "linux" {
        "bin/java"
    } else {
        "bin/java.exe"
    };
    json!({"files": {"bin": {"type": "directory"}, (java): {"type": "file", "executable": true, "downloads": {"raw": payload(RAW)}}}})
}

fn parse_fixture(value: &Value, platform: &str) -> Result<Package, Error> {
    let bytes = value.to_string();
    Package::parse(
        bytes.as_bytes(),
        &sha1(bytes.as_bytes()),
        Some(bytes.len() as u64),
        platform,
    )
}

#[test]
fn checks_runtime_manifest_integrity_and_platform_launcher() {
    let doc = fixture("windows-x64");
    assert!(parse_fixture(&doc, "windows-x64").is_ok());
    assert!(parse_fixture(&doc, "linux").is_err());
    assert!(matches!(
        Package::parse(doc.to_string().as_bytes(), &[0; 20], None, "windows-x64"),
        Err(Error::Java("runtime manifest integrity mismatch"))
    ));
    let raw = doc.to_string();
    assert!(matches!(
        Package::parse(
            raw.as_bytes(),
            &sha1(raw.as_bytes()),
            Some(1),
            "windows-x64"
        ),
        Err(Error::Java("runtime manifest integrity mismatch"))
    ));
}

#[test]
fn rejects_path_escape_case_conflicts_and_link_chains() {
    for (path, entry) in [
        ("../outside", json!({"type":"directory"})),
        ("bin/Java", json!({"type":"directory"})),
        ("other/child", json!({"type":"directory"})),
        (
            "bin/unsafe",
            json!({"type":"link","target":"../../outside"}),
        ),
        ("bin/unsafe", json!({"type":"link","target":"/bin/java"})),
        ("bin/unsafe", json!({"type":"link","target":"java/missing"})),
        ("bin/unsafe", json!({"type":"link","target":"unsafe"})),
    ] {
        let mut doc = fixture("linux");
        doc["files"]
            .as_object_mut()
            .unwrap()
            .insert(path.into(), entry);
        assert!(parse_fixture(&doc, "linux").is_err(), "{path}");
    }
    let mut doc = fixture("linux");
    doc["files"]["legal"] = json!({"type":"directory"});
    doc["files"]["legal/java"] = json!({"type":"link","target":"../bin/java"});
    assert!(parse_fixture(&doc, "linux").is_ok());
}

#[test]
fn accepts_the_legacy_runtime_high_resolution_splash_filename() {
    let mut doc = fixture("linux");
    doc["files"]["lib"] = json!({"type":"directory"});
    doc["files"]["lib/splash@2x.gif"] =
        json!({"type":"file","executable":false,"downloads":{"raw":payload(RAW)}});
    assert!(parse_fixture(&doc, "linux").is_ok());
}

#[test]
fn decodes_lzma_with_final_size_and_checksum_verification() {
    let base = tempfile::tempdir().unwrap();
    let archive = base.path().join("packed");
    fs::write(&archive, LZMA).unwrap();
    let target = base.path().join("runtime");

    for (size, hash, error) in [
        (RAW.len() as u64 + 1, sha1(RAW), Some(Failure::SizeMismatch)),
        (RAW.len() as u64, [0; 20], Some(Failure::ChecksumMismatch)),
        (RAW.len() as u64, sha1(RAW), None),
    ] {
        let mut item: ArtifactJson = serde_json::from_value(payload(RAW)).unwrap();
        item.size = size;
        item.sha1 = hex_sha1(&hash);
        let file = unpack_download(
            artifact(item).unwrap(),
            "Java fixture".into(),
            target.clone(),
            archive.clone(),
        );
        let (_sender, cancel) = download::cancellation();
        let result = block_on(download::run(&[file], cancel, |_| future::ready(())));
        if let Some(error) = error {
            assert_eq!(result.unwrap_err().failure, error);
            assert!(!target.exists());
        } else {
            assert_eq!(result.unwrap(), [ArtifactStatus::Verified]);
            assert_eq!(fs::read(&target).unwrap(), RAW);
        }
    }
}

#[test]
fn rejects_oversized_lzma_dictionary_before_publishing() {
    let base = tempfile::tempdir().unwrap();
    let archive = base.path().join("packed");
    let mut bytes = LZMA.to_vec();
    bytes[1..5].copy_from_slice(&(256_u32 * 1024 * 1024).to_le_bytes());
    fs::write(&archive, bytes).unwrap();
    let target = base.path().join("runtime");
    let item = serde_json::from_value(payload(RAW)).unwrap();
    let file = unpack_download(
        artifact(item).unwrap(),
        "Java fixture".into(),
        target.clone(),
        archive,
    );
    let (_sender, cancel) = download::cancellation();
    let error = block_on(download::run(&[file], cancel, |_| future::ready(()))).unwrap_err();
    assert!(matches!(error.failure, Failure::Network(_)));
    assert!(!target.exists());
}

#[cfg(unix)]
#[test]
fn an_incomplete_runtime_is_not_reused_and_links_do_not_replace_files() {
    use std::os::unix::fs::PermissionsExt;

    let base = tempfile::tempdir().unwrap();
    let mut doc = fixture("linux");
    doc["files"]["bin/control"] = json!({"type":"link","target":"java"});
    let slot = base.path();
    let bin = slot.join("runtime/bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(bin.join("java"), RAW).unwrap();
    let (_sender, cancel) = download::cancellation();
    let report = |_: Progress| future::ready(()).boxed();
    let package = parse_fixture(&doc, "linux").unwrap();
    assert_eq!(package.ready(slot, &cancel).unwrap(), None);
    let executable =
        block_on(package.install(slot, 17, &Https::default(), &cancel, &report)).unwrap();
    assert_eq!(
        fs::metadata(&executable).unwrap().permissions().mode() & 0o100,
        0o100
    );
    assert_eq!(
        fs::read_link(bin.join("control")).unwrap(),
        Path::new("java")
    );
    let package = parse_fixture(&doc, "linux").unwrap();
    assert_eq!(package.ready(slot, &cancel).unwrap(), Some(executable));
    fs::remove_file(bin.join("control")).unwrap();
    fs::write(bin.join("control"), b"user data").unwrap();
    let result = block_on(package.install(slot, 17, &Https::default(), &cancel, &report));
    assert_eq!(
        result,
        Err(Error::Java("conflicting runtime link was preserved"))
    );
    assert_eq!(fs::read(bin.join("control")).unwrap(), b"user data");
}
