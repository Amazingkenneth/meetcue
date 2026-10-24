//! API keys live only in the system credential store (Windows Credential
//! Manager, macOS Keychain, Secret Service on Linux). They are never written
//! to the settings file or the logs, and never sent to the webview.

use std::collections::HashMap;
use std::sync::Mutex;

use meetcue_core::{codes, MeetCueError};

const SERVICE: &str = "MeetCue";

pub trait CredentialStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, MeetCueError>;
    fn set(&self, key: &str, secret: &str) -> Result<(), MeetCueError>;
    fn delete(&self, key: &str) -> Result<(), MeetCueError>;

    fn has(&self, key: &str) -> bool {
        matches!(self.get(key), Ok(Some(s)) if !s.is_empty())
    }
}

/// Credential name for a provider secret, e.g. `soniox` or `azure-speech.region`.
pub fn key_for(provider: &str, field: &str) -> String {
    if field == "apiKey" {
        provider.to_string()
    } else {
        format!("{provider}.{field}")
    }
}

pub struct SystemCredentials;

fn store_err(e: keyring::Error) -> MeetCueError {
    MeetCueError::new(codes::SYS_CREDENTIALS, e.to_string())
}

impl CredentialStore for SystemCredentials {
    fn get(&self, key: &str) -> Result<Option<String>, MeetCueError> {
        let entry = keyring::Entry::new(SERVICE, key).map_err(store_err)?;
        match entry.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(store_err(e)),
        }
    }

    fn set(&self, key: &str, secret: &str) -> Result<(), MeetCueError> {
        let entry = keyring::Entry::new(SERVICE, key).map_err(store_err)?;
        entry.set_password(secret.trim()).map_err(store_err)
    }

    fn delete(&self, key: &str) -> Result<(), MeetCueError> {
        let entry = keyring::Entry::new(SERVICE, key).map_err(store_err)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(store_err(e)),
        }
    }
}

/// In-memory store for tests and demo mode.
#[derive(Default)]
pub struct MemoryCredentials {
    map: Mutex<HashMap<String, String>>,
}

impl CredentialStore for MemoryCredentials {
    fn get(&self, key: &str) -> Result<Option<String>, MeetCueError> {
        Ok(self.map.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, secret: &str) -> Result<(), MeetCueError> {
        self.map
            .lock()
            .unwrap()
            .insert(key.into(), secret.trim().into());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), MeetCueError> {
        self.map.lock().unwrap().remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store() {
        let s = MemoryCredentials::default();
        assert!(!s.has("soniox"));
        s.set("soniox", "  abc \n").unwrap();
        assert_eq!(s.get("soniox").unwrap().as_deref(), Some("abc"));
        assert!(s.has("soniox"));
        s.delete("soniox").unwrap();
        assert!(!s.has("soniox"));
        assert_eq!(key_for("azure-speech", "region"), "azure-speech.region");
        assert_eq!(key_for("anthropic", "apiKey"), "anthropic");
    }
}
