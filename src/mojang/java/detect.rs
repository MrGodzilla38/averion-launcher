use super::Runtime;
use crate::download::{Cancellation, Failure};
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};

pub(super) const BIN: &str = if cfg!(windows) { "java.exe" } else { "java" };
const PROBE_TIMEOUT: Duration = Duration::from_secs(4);
const MAX_OUTPUT: u64 = 32 * 1024;

pub(super) async fn system_java(
    major: u16,
    managed_root: &Path,
    cancel: &Cancellation,
) -> Result<Option<Runtime>, Failure> {
    let managed_root = managed_root
        .canonicalize()
        .unwrap_or_else(|_| managed_root.to_owned());
    let mut visited = HashSet::new();
    for candidate in candidates() {
        cancel.check()?;
        let Ok(path) = candidate.canonicalize() else {
            continue;
        };
        if path.starts_with(&managed_root) || !visited.insert(path.clone()) {
            continue;
        }
        if let Some(runtime) = compatible(path, major, cancel).await? {
            return Ok(Some(runtime));
        }
    }
    Ok(None)
}

fn candidates() -> Vec<PathBuf> {
    let mut result = Vec::new();
    for name in ["JAVA_HOME", "JDK_HOME", "JRE_HOME"] {
        if let Some(home) = env::var_os(name)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
        {
            result.push(home.join("bin").join(BIN));
        }
    }
    if let Some(path) = env::var_os("PATH") {
        result.extend(
            env::split_paths(&path)
                .filter(|p| p.is_absolute())
                .map(|bin| bin.join(BIN)),
        );
    }
    #[cfg(target_os = "linux")]
    for root in ["/usr/lib/jvm", "/usr/lib64/jvm", "/usr/java"] {
        add_homes(Path::new(root), &mut result);
    }
    #[cfg(windows)]
    {
        const VENDORS: &[&str] = &[
            "Java",
            "Eclipse Adoptium",
            "Microsoft",
            "Amazon Corretto",
            "Zulu",
            "BellSoft",
        ];
        for base in ["ProgramW6432", "ProgramFiles", "LOCALAPPDATA"] {
            let Some(mut root) = env::var_os(base)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
            else {
                continue;
            };
            if base == "LOCALAPPDATA" {
                root.push("Programs");
            }
            for vendor in VENDORS {
                add_homes(&root.join(vendor), &mut result);
            }
        }
    }
    result
}

fn add_homes(root: &Path, result: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(root) {
        let mut homes: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
        homes.sort();
        result.extend(homes.into_iter().map(|home| home.join("bin").join(BIN)));
    }
}

pub(super) async fn compatible(
    executable: PathBuf,
    major: u16,
    cancel: &Cancellation,
) -> Result<Option<Runtime>, Failure> {
    cancel.check()?;
    if !executable.is_absolute() || !native_binary(&executable).unwrap_or(false) {
        return Ok(None);
    }
    let output = cancel
        .wait(tokio::time::timeout(PROBE_TIMEOUT, probe(&executable)))
        .await?;
    if matches!(output, Ok(Ok(found)) if found == Some(major)) {
        Ok(Some(Runtime { executable, major }))
    } else {
        Ok(None)
    }
}

async fn probe(executable: &Path) -> std::io::Result<Option<u16>> {
    let scratch = tempfile::tempdir()?;
    let mut command = Command::new(executable);
    command
        .args(["-XshowSettings:properties", "-version"])
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .current_dir(scratch.path());
    for variable in [
        "JAVA_TOOL_OPTIONS",
        "_JAVA_OPTIONS",
        "JDK_JAVA_OPTIONS",
        "CLASSPATH",
        "IBM_JAVA_OPTIONS",
        "OPENJ9_JAVA_OPTIONS",
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "LD_AUDIT",
    ] {
        command.env_remove(variable);
    }
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW

    let mut child = command.spawn()?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    // Read both streams concurrently so neither pipe can stall the process.
    let (stdout, stderr) = futures::try_join!(read_output(stdout), read_output(stderr))?;
    if stdout.len() as u64 > MAX_OUTPUT
        || stderr.len() as u64 > MAX_OUTPUT
        || !child.wait().await?.success()
    {
        return Ok(None);
    }
    Ok(parse(&stdout, &stderr))
}

async fn read_output(pipe: impl tokio::io::AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    pipe.take(MAX_OUTPUT + 1).read_to_end(&mut output).await?;
    Ok(output)
}

fn native_binary(path: &Path) -> std::io::Result<bool> {
    use std::io::Read;

    if !path.is_file() {
        return Ok(false);
    }
    let mut file = fs::File::open(path)?;
    #[cfg(target_os = "linux")]
    {
        let mut header = [0; 20];
        file.read_exact(&mut header)?;
        Ok(&header[..6] == b"\x7fELF\x02\x01" && header[18..20] == [0x3e, 0])
    }
    #[cfg(windows)]
    {
        use std::io::{Seek, SeekFrom};

        let mut header = [0; 64];
        file.read_exact(&mut header)?;
        let offset = u32::from_le_bytes(header[60..64].try_into().unwrap());
        if &header[..2] != b"MZ" || !(64..=1024 * 1024).contains(&offset) {
            return Ok(false);
        }
        file.seek(SeekFrom::Start(u64::from(offset)))?;
        let mut signature = [0; 6];
        file.read_exact(&mut signature)?;
        Ok(signature == *b"PE\0\0\x64\x86")
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    Ok(false)
}

fn parse(stdout: &[u8], stderr: &[u8]) -> Option<u16> {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    let property = |key: &str| {
        stdout.lines().chain(stderr.lines()).find_map(|line| {
            let (name, value) = line.trim().split_once('=')?;
            (name.trim() == key).then(|| value.trim())
        })
    };
    if !matches!(property("os.arch")?, "amd64" | "x86_64" | "x64")
        || property("sun.arch.data.model").is_some_and(|model| model != "64")
    {
        return None;
    }
    version_major(property("java.version")?)
}

pub(super) fn version_major(version: &str) -> Option<u16> {
    if version.contains("-ea") || version.contains("-internal") {
        return None;
    }
    let version = version.strip_prefix("1.").unwrap_or(version);
    let end = version
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(version.len());
    if !version[end..].starts_with(['.', '_', 'u', '+', '-']) && end != version.len() {
        return None;
    }
    version[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_legacy_and_modern_java_versions() {
        for (version, major) in [
            ("1.8.0_202", 8),
            ("8u51-cacert", 8),
            ("16.0.1.9.1", 16),
            ("17.0.15", 17),
            ("21.0.7+6", 21),
            ("25", 25),
        ] {
            assert_eq!(version_major(version), Some(major));
        }
        assert_eq!(version_major("25-ea+1"), None);
        assert_eq!(version_major("java17"), None);
    }

    #[test]
    fn probes_require_a_64_bit_x86_vm() {
        for (arch, model, expected) in [
            ("amd64", "64", Some(17)),
            ("x86_64", "64", Some(17)),
            ("x86", "32", None),
            ("aarch64", "64", None),
            ("amd64", "32", None),
        ] {
            let properties = format!(
                "java.version = 17.0.15\nos.arch = {arch}\nsun.arch.data.model = {model}\n"
            );
            assert_eq!(parse(&[], properties.as_bytes()), expected);
        }
        assert_eq!(parse(&[], b"openjdk version \"17\""), None);
    }
}
