//! Where a site's refresh token is kept (B4): the OS credential store
//! (Windows Credential Manager through `keyring`), service `foqa-sim`,
//! account the origin. Behind a trait so tests never touch the real store.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::lock;

pub trait SecretStore: Send + Sync {
    fn get(&self, origin: &str) -> Result<Option<String>, String>;
    fn set(&self, origin: &str, secret: &str) -> Result<(), String>;
    fn delete(&self, origin: &str) -> Result<(), String>;
}

const SERVICE: &str = "foqa-sim";

/// The OS credential store.
pub struct KeyringStore;

impl KeyringStore {
    fn entry(origin: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, origin).map_err(|e| e.to_string())
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, origin: &str) -> Result<Option<String>, String> {
        match Self::entry(origin)?.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, origin: &str, secret: &str) -> Result<(), String> {
        Self::entry(origin)?
            .set_password(secret)
            .map_err(|e| e.to_string())
    }

    fn delete(&self, origin: &str) -> Result<(), String> {
        match Self::entry(origin)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Kept in memory only: tests, and the session when the OS store fails
/// (never a file).
#[derive(Default)]
pub struct MemoryStore {
    secrets: Mutex<HashMap<String, String>>,
    broken: bool,
}

impl MemoryStore {
    /// A store whose `set` fails, as an unavailable OS store does.
    pub fn broken() -> Self {
        Self {
            broken: true,
            ..Default::default()
        }
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, origin: &str) -> Result<Option<String>, String> {
        Ok(lock(&self.secrets).get(origin).cloned())
    }

    fn set(&self, origin: &str, secret: &str) -> Result<(), String> {
        if self.broken {
            return Err("credential store unavailable".into());
        }
        lock(&self.secrets).insert(origin.into(), secret.into());
        Ok(())
    }

    fn delete(&self, origin: &str) -> Result<(), String> {
        lock(&self.secrets).remove(origin);
        Ok(())
    }
}
