//! Public account identities, their active selection, and Minecraft launch sessions.

pub(crate) mod microsoft;
mod vault;

use crate::game_directory::GameDirectory;
use serde::{Deserialize, Serialize};
use std::{
    fmt, fs,
    io::{self, Read, Write},
};
use zeroize::Zeroizing;

/// Placeholder access token for offline play.
///
/// The Minecraft client only forwards this value to services that require an
/// authenticated (online-mode) session, which offline play cannot use anyway.
/// `"0"` is a well-known non-secret offline convention. It is never a
/// Microsoft token and must never be logged as one.
const OFFLINE_TOKEN: &str = "0";

const MIN_NAME: usize = 3;
const MAX_NAME: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Auth {
    Offline,
    Microsoft,
}

impl Auth {
    pub fn label(self) -> &'static str {
        match self {
            Self::Offline => "Offline",
            Self::Microsoft => "Microsoft",
        }
    }
}

/// An account's public display identity. Secrets live in the vault or a launch session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    name: String,
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

/// The values passed to Minecraft. A session deliberately has no debug representation.
pub struct Session {
    pub player_name: String,
    pub uuid: String,
    pub access_token: Zeroizing<String>,
    pub user_type: &'static str,
    pub client_id: String,
    pub xuid: String,
}

impl Account {
    pub fn offline(name: &str) -> Result<Self, NameError> {
        validate_name(name)?;
        Ok(Self {
            name: name.to_owned(),
            uuid: offline_uuid(name),
            auth: Auth::Offline,
        })
    }

    pub(crate) fn microsoft(name: String, uuid: &str) -> Option<Self> {
        validate_name(&name).ok()?;
        Some(Self {
            name,
            uuid: canonical_uuid(uuid)?,
            auth: Auth::Microsoft,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn auth(&self) -> Auth {
        self.auth
    }

    fn same_identity(&self, other: &Self) -> bool {
        self.auth == other.auth && self.uuid == other.uuid
    }

    fn offline_session(&self) -> Option<Session> {
        (self.auth == Auth::Offline).then(|| Session {
            player_name: self.name.clone(),
            uuid: self.uuid.clone(),
            access_token: Zeroizing::new(OFFLINE_TOKEN.to_owned()),
            user_type: "legacy",
            client_id: String::new(),
            xuid: String::new(),
        })
    }
}

fn canonical_uuid(value: &str) -> Option<String> {
    let value = match value.len() {
        32 => value.to_owned(),
        36 if value.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        }) =>
        {
            value.replace('-', "")
        }
        _ => return None,
    };
    if !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let value = value.to_ascii_lowercase();
    Some(format!(
        "{}-{}-{}-{}-{}",
        &value[..8],
        &value[8..12],
        &value[12..16],
        &value[16..20],
        &value[20..]
    ))
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

const ACCOUNT_LIMIT: usize = 64;
const FILE_LIMIT: usize = 32 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accounts {
    pub entries: Vec<Account>,
    active: Option<usize>,
}

impl Accounts {
    pub fn active(&self) -> Option<&Account> {
        self.active.and_then(|index| self.entries.get(index))
    }

    pub fn is_active(&self, index: usize) -> bool {
        self.active == Some(index)
    }

    pub(crate) fn update_profile(&mut self, profile: Account) {
        if let Some(index) = self.index_of(&profile) {
            self.entries[index] = profile;
        }
    }

    fn index_of(&self, account: &Account) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.same_identity(account))
    }

    fn insert(&mut self, account: Account) -> io::Result<()> {
        if let Some(index) = self.index_of(&account) {
            self.entries[index] = account;
            self.active = Some(index);
        } else {
            if self.entries.len() >= ACCOUNT_LIMIT {
                return Err(io::Error::other("The saved account limit was reached"));
            }
            self.active = Some(self.entries.len());
            self.entries.push(account);
        }
        Ok(())
    }

    fn validate(&self) -> bool {
        self.entries.len() <= ACCOUNT_LIMIT
            && self.active.is_none_or(|index| index < self.entries.len())
            && self.entries.iter().enumerate().all(|(index, account)| {
                let valid = match account.auth {
                    Auth::Offline => {
                        Account::offline(&account.name).is_ok_and(|expected| expected == *account)
                    }
                    Auth::Microsoft => Account::microsoft(account.name.clone(), &account.uuid)
                        .is_some_and(|expected| expected == *account),
                };
                valid && self.index_of(account) == Some(index)
            })
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Stored {
    Current(Accounts),
    Legacy(Legacy),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Legacy {
    kind: String,
    username: String,
}

pub fn load(directory: &GameDirectory) -> io::Result<Accounts> {
    let path = directory.account_file();
    let mut bytes = Vec::new();
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !crate::game_directory::is_link(&metadata) => {
            fs::File::open(&path)?
                .take(FILE_LIMIT as u64 + 1)
                .read_to_end(&mut bytes)?;
        }
        Ok(_) => return Err(io::ErrorKind::InvalidInput.into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Accounts::default()),
        Err(error) => return Err(error),
    }
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "The saved accounts file is invalid",
        )
    };
    if bytes.len() > FILE_LIMIT {
        return Err(invalid());
    }
    match serde_json::from_slice(&bytes).map_err(|_| invalid())? {
        Stored::Current(accounts) if accounts.validate() => Ok(accounts),
        Stored::Legacy(legacy) if legacy.kind == "offline" => {
            let mut accounts = Accounts::default();
            accounts.insert(Account::offline(&legacy.username).map_err(|_| invalid())?)?;
            Ok(accounts)
        }
        _ => Err(invalid()),
    }
}

fn write(directory: &GameDirectory, accounts: &Accounts) -> io::Result<()> {
    directory.prepare_config()?;
    let path = directory.account_file();
    let document = serde_json::to_vec(accounts).map_err(|_| io::ErrorKind::InvalidData)?;
    let parent = path.parent().ok_or(ErrorKind::InvalidInput)?;
    let mut stage = tempfile::Builder::new()
        .prefix(".averion-")
        .suffix(".part")
        .tempfile_in(parent)?;
    stage.write_all(&document)?;
    stage.write_all(b"\n")?;
    stage.as_file().sync_all()?;
    stage.persist(&path).map_err(|error| error.error)?;
    Ok(())
}

pub fn save_offline(directory: &GameDirectory, username: &str) -> io::Result<Accounts> {
    let account = Account::offline(username)
        .map_err(|error| io::Error::new(ErrorKind::InvalidInput, error))?;
    let _lock = directory.lock_accounts()?;
    let mut accounts = load(directory)?;
    accounts.insert(account)?;
    write(directory, &accounts)?;
    Ok(accounts)
}

pub fn select(directory: &GameDirectory, account: &Account) -> io::Result<Accounts> {
    let _lock = directory.lock_accounts()?;
    let mut accounts = load(directory)?;
    accounts.active = Some(accounts.index_of(account).ok_or(ErrorKind::NotFound)?);
    write(directory, &accounts)?;
    Ok(accounts)
}

pub fn remove(directory: &GameDirectory, account: &Account) -> Result<Accounts, microsoft::Error> {
    let _lock = directory.lock_accounts()?;
    let mut accounts = load(directory)?;
    let index = accounts
        .index_of(account)
        .ok_or_else(|| io::Error::from(ErrorKind::NotFound))?;
    if account.auth == Auth::Microsoft {
        vault::Native.remove(&account.uuid)?;
    }
    accounts.entries.remove(index);
    accounts.active = accounts.active.and_then(|active| match active.cmp(&index) {
        std::cmp::Ordering::Less => Some(active),
        std::cmp::Ordering::Equal => None,
        std::cmp::Ordering::Greater => Some(active - 1),
    });
    write(directory, &accounts)?;
    Ok(accounts)
}

use std::io::ErrorKind;
use vault::Vault;

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
        let session = Account::offline("Notch")
            .unwrap()
            .offline_session()
            .unwrap();
        assert_eq!(session.player_name, "Notch");
        assert_eq!(session.uuid, "b50ad385-829d-3141-a216-7e7d7539ba7f");
        assert_eq!(session.access_token.as_str(), "0");
        assert_eq!(session.user_type, "legacy");
        assert!(session.client_id.is_empty());
        assert!(session.xuid.is_empty());
        assert_eq!(Account::offline("Notch").unwrap().auth, Auth::Offline);
    }

    #[test]
    fn account_file_round_trips_and_rejects_unsupported_documents() {
        let base = tempfile::tempdir().unwrap();
        let directory = GameDirectory::for_test(base.path());
        assert!(load(&directory).unwrap().active().is_none());
        directory.prepare_config().unwrap();
        fs::write(
            directory.account_file(),
            br#"{"kind":"offline","username":"Notch"}"#,
        )
        .unwrap();
        assert_eq!(load(&directory).unwrap().active().unwrap().name(), "Notch");

        let saved = save_offline(&directory, "Steve_123").unwrap();
        assert_eq!(load(&directory).unwrap(), saved);
        assert_eq!(saved.entries.len(), 2);
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
            save_offline(&directory, "no spaces").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
