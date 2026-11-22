use super::microsoft::Error;
use keyring_core::{CredentialStore, Entry};
use std::{collections::HashMap, sync::Arc};
use zeroize::Zeroizing;

const SERVICE: &str = "io.github.MrGodzilla38.averion-launcher.microsoft-refresh";

pub(super) trait Vault {
    fn read(&self, uuid: &str) -> Result<Zeroizing<String>, Error>;
    fn write(&self, uuid: &str, refresh: &str) -> Result<(), Error>;
    fn remove(&self, uuid: &str) -> Result<(), Error>;
}

pub(super) struct Native;

fn entry(uuid: &str) -> Result<Entry, Error> {
    #[cfg(target_os = "linux")]
    let store: Arc<CredentialStore> =
        zbus_secret_service_keyring_store::Store::new().map_err(vault_error)?;
    #[cfg(windows)]
    let store: Arc<CredentialStore> =
        windows_native_keyring_store::Store::new().map_err(vault_error)?;
    #[cfg(target_os = "linux")]
    let modifiers = HashMap::new();
    #[cfg(windows)]
    let modifiers = HashMap::from([("persistence", "Local")]);
    Entry::new_in_store_with_modifiers(&store, SERVICE, uuid, &modifiers).map_err(vault_error)
}

fn vault_error(error: keyring_core::Error) -> Error {
    match error {
        keyring_core::Error::NoEntry => Error::SignInAgain,
        keyring_core::Error::TooLong(_, _) => Error::VaultCapacity,
        _ => Error::VaultUnavailable,
    }
}

impl Vault for Native {
    fn read(&self, uuid: &str) -> Result<Zeroizing<String>, Error> {
        entry(uuid)?
            .get_password()
            .map(Zeroizing::new)
            .map_err(vault_error)
    }

    fn write(&self, uuid: &str, refresh: &str) -> Result<(), Error> {
        entry(uuid)?
            .set_secret(refresh.as_bytes())
            .map_err(vault_error)
    }

    fn remove(&self, uuid: &str) -> Result<(), Error> {
        match entry(uuid)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(vault_error(error)),
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires an unlocked graphical Secret Service session"]
    fn native_secret_service_round_trip() {
        let mut nonce = [0; 8];
        getrandom::fill(&mut nonce).unwrap();
        let user = format!("averion-test-{:016x}", u64::from_ne_bytes(nonce));
        let vault = Native;
        vault.write(&user, "synthetic-test-credential").unwrap();
        let value = vault.read(&user);
        let cleanup = vault.remove(&user);
        cleanup.unwrap();
        assert_eq!(value.unwrap().as_str(), "synthetic-test-credential");
        assert!(matches!(vault.read(&user), Err(Error::SignInAgain)));
    }
}
