//! Account sessions consumed by the launch engine.
//!
//! Offline accounts are fully implemented: Mojang-style name validation,
//! deterministic offline UUIDs and the argument values the client expects for
//! offline play. Microsoft authentication is intentionally **not** implemented
//! in this phase. [`Auth`] is the seam where a future OS-vault-backed
//! Microsoft token provider attaches; the launch engine already consumes
//! [`Account::session`] regardless of how the account was produced.

use crate::game_directory::GameDirectory;
use std::{
    fmt, fs,
    io::{self, Read, Write},
};

/// Placeholder access token for offline play.
///
/// The Minecraft client only forwards this value to services that require an
/// authenticated (online-mode) session, which offline play cannot use anyway.
/// `"0"` is a well-known non-secret offline convention. It is never a
/// Microsoft token and must never be logged as one.
const OFFLINE_TOKEN: &str = "0";

const MIN_NAME: usize = 3;
const MAX_NAME: usize = 16;

/// How the session was authenticated. A future `Microsoft` variant carries a
/// real token held in operating-system credential storage; it is deliberately
/// absent until that flow exists, so no placeholder token can be mistaken for
/// a working Microsoft session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Auth {
    Offline,
}

/// A player identity the launch engine can use.
///
/// Offline accounts contain no secrets, so deriving `Debug` is safe. A future
/// Microsoft variant must hold its token in a redacting wrapper instead,
/// because `Message` and its payloads are formatted during diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    name: String,
    /// Canonical lowercase hyphenated UUID.
    uuid: String,
    auth: Auth,
}

/// Why an offline username was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    Length,
    Characters,
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length => write!(f, "names must be {MIN_NAME} to {MAX_NAME} characters long"),
            Self::Characters => f.write_str("names may only contain letters, numbers and _"),
        }
    }
}

impl std::error::Error for NameError {}

/// The account values substituted into Minecraft's launch arguments.
///
/// Every field is safe to derive into argument values. Offline sessions never
/// contain credentials: `access_token` is the public placeholder described in
/// [`Auth`], and `client_id`/`xuid` are empty because no Microsoft identity is
/// involved.
pub struct Session {
    pub player_name: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
    pub client_id: String,
    pub xuid: String,
}

impl Account {
    /// Builds a validated offline account with a deterministic offline UUID.
    pub fn offline(name: &str) -> Result<Self, NameError> {
        validate_name(name)?;
        Ok(Self {
            name: name.to_owned(),
            uuid: offline_uuid(name),
            auth: Auth::Offline,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Produces the argument values for a launch. The Microsoft flow will
    /// supply a real token here without changing the launch engine.
    pub fn session(&self) -> Session {
        match self.auth {
            Auth::Offline => Session {
                player_name: self.name.clone(),
                uuid: self.uuid.clone(),
                access_token: OFFLINE_TOKEN.to_owned(),
                // The client accepts "legacy" for sessions without a Microsoft
                // account; offline play is never "msa".
                user_type: "legacy".to_owned(),
                client_id: String::new(),
                xuid: String::new(),
            },
        }
    }
}

/// Validates an offline username against Mojang's historical rules.
pub fn validate_name(name: &str) -> Result<(), NameError> {
    if (MIN_NAME..=MAX_NAME).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        Ok(())
    } else if name.len() < MIN_NAME || name.len() > MAX_NAME {
        Err(NameError::Length)
    } else {
        Err(NameError::Characters)
    }
}

/// Deterministic version-3 UUID derived from `OfflinePlayer:<name>`, matching
/// Java's `UUID.nameUUIDFromBytes` as used by offline Minecraft servers.
fn offline_uuid(name: &str) -> String {
    use md5::{Digest, Md5};

    let mut hash = Md5::new();
    hash.update(b"OfflinePlayer:");
    hash.update(name.as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&hash.finalize());
    // RFC 4122 version 3 and IETF variant bits, as Java's nameUUIDFromBytes sets.
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = |bytes: &[u8]| {
        let mut value = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            use std::fmt::Write;
            write!(value, "{byte:02x}").expect("writing to a String cannot fail");
        }
        value
    };
    format!(
        "{}-{}-{}-{}-{}",
        hex(&bytes[..4]),
        hex(&bytes[4..6]),
        hex(&bytes[6..8]),
        hex(&bytes[8..10]),
        hex(&bytes[10..]),
    )
}

/// Reads the persisted account, if one has been configured. Malformed or
/// unsupported files are reported instead of silently ignored.
pub fn load(directory: &GameDirectory) -> io::Result<Option<Account>> {
    let path = directory.account_file();
    let bytes = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !crate::game_directory::is_link(&metadata) => {
            let mut bytes = Vec::new();
            fs::File::open(&path)?.take(4097).read_to_end(&mut bytes)?;
            bytes
        }
        Ok(_) => return Err(io::ErrorKind::InvalidInput.into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if bytes.len() > 4096 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let account: Stored = serde_json::from_slice(&bytes).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "account file is not valid JSON")
    })?;
    if account.kind != "offline" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported account kind",
        ));
    }
    let name = account.username;
    let account = Account::offline(&name).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("stored offline username is invalid ({error})"),
        )
    })?;
    Ok(Some(account))
}

/// Validates and atomically stores the offline account. No token or secret is
/// written; only the username is persisted.
pub fn save(directory: &GameDirectory, username: &str) -> io::Result<Account> {
    let account = Account::offline(username).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid username ({error})"),
        )
    })?;
    directory.prepare_config()?;
    let path = directory.account_file();
    if let Ok(metadata) = fs::symlink_metadata(&path)
        && (crate::game_directory::is_link(&metadata) || !metadata.is_file())
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let document = serde_json::to_vec(&Stored {
        kind: "offline".into(),
        username: account.name().into(),
    })
    .map_err(|_| io::ErrorKind::InvalidData)?;
    let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let mut stage = tempfile::Builder::new()
        .prefix(".averion-")
        .suffix(".part")
        .tempfile_in(parent)?;
    stage.write_all(&document)?;
    stage.write_all(b"\n")?;
    stage.as_file().sync_all()?;
    stage.persist(&path).map_err(|error| error.error)?;
    Ok(account)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Stored {
    kind: String,
    username: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_names_follow_mojang_rules() {
        for valid in ["abc", "Player", "Steve_123", "A1b2C3d4E5f6G7h8"] {
            assert!(validate_name(valid).is_ok(), "{valid}");
        }
        for (invalid, expected) in [
            ("ab", NameError::Length),
            ("A1b2C3d4E5f6G7h8a", NameError::Length),
            ("with space", NameError::Characters),
            ("bad-name", NameError::Characters),
            ("évil", NameError::Characters),
            ("", NameError::Length),
        ] {
            assert_eq!(validate_name(invalid), Err(expected), "{invalid}");
            assert!(Account::offline(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn offline_uuids_match_java_name_uuid_from_bytes() {
        // Expected values were computed with Python's uuid.UUID(..., version=3)
        // over MD5("OfflinePlayer:<name>"), the same construction Java uses.
        for (name, expected) in [
            ("Notch", "b50ad385-829d-3141-a216-7e7d7539ba7f"),
            ("Player", "a01e3843-e521-3998-958a-f459800e4d11"),
            ("Steve_123", "77cae0ff-71ba-39ba-b80c-74428607dfcc"),
            ("abc", "3d5cec06-bd15-31fa-982f-5dac8c06f1c7"),
            ("A1b2C3d4E5f6G7h8", "dd2d3333-160e-3398-9bbf-1653c187bdb4"),
        ] {
            assert_eq!(Account::offline(name).unwrap().uuid, expected, "{name}");
        }
    }

    #[test]
    fn offline_session_carries_no_microsoft_credentials() {
        let session = Account::offline("Notch").unwrap().session();
        assert_eq!(session.player_name, "Notch");
        assert_eq!(session.uuid, "b50ad385-829d-3141-a216-7e7d7539ba7f");
        assert_eq!(session.access_token, "0");
        assert_eq!(session.user_type, "legacy");
        assert!(session.client_id.is_empty());
        assert!(session.xuid.is_empty());
        assert_eq!(Account::offline("Notch").unwrap().auth, Auth::Offline);
    }

    #[test]
    fn account_file_round_trips_and_rejects_unsupported_documents() {
        let base = tempfile::tempdir().unwrap();
        let directory = GameDirectory::for_test(base.path());
        assert!(load(&directory).unwrap().is_none());

        let saved = save(&directory, "Steve_123").unwrap();
        assert_eq!(load(&directory).unwrap().as_ref(), Some(&saved));
        let document = fs::read(directory.account_file()).unwrap();
        let text = String::from_utf8(document).unwrap();
        assert!(text.contains("\"offline\""));
        // Only the username is persisted — never a token.
        assert!(!text.contains("token"));

        fs::write(directory.account_file(), b"{\"kind\":\"microsoft\"}").unwrap();
        assert_eq!(
            load(&directory).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::write(directory.account_file(), b"not json").unwrap();
        assert_eq!(
            load(&directory).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            save(&directory, "no spaces").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
