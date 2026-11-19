use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::{Error, SERVICE, Stored};

const ACCOUNT: &str = "credentials";

static STORE: Mutex<CredentialStore<Keychain>> = Mutex::new(CredentialStore {
    backend: Keychain,
    cache: None,
});

#[derive(Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    entries: BTreeMap<String, String>,
}

trait Backend {
    fn read(&self) -> Result<Stored<String>, Error>;
    fn save(&self, value: &str) -> Result<(), Error>;
}

struct Keychain;

impl Backend for Keychain {
    fn read(&self) -> Result<Stored<String>, Error> {
        super::system::read_entry(ACCOUNT)
    }

    fn save(&self, value: &str) -> Result<(), Error> {
        keyring::Entry::new(SERVICE, ACCOUNT)?.set_password(value)?;
        Ok(())
    }
}

/// All credentials live in one Keychain item, read once per process and cached.
struct CredentialStore<B> {
    backend: B,
    cache: Option<Credentials>,
}

impl<B: Backend> CredentialStore<B> {
    fn load(&mut self) -> Result<&mut Credentials, Error> {
        let credentials = match self.cache.take() {
            Some(credentials) => credentials,
            None => match self.backend.read()? {
                Stored::Ready(encoded) => {
                    serde_json::from_str(&encoded).map_err(|_| Error::InvalidStore)?
                }
                Stored::Missing => Credentials::default(),
            },
        };
        Ok(self.cache.insert(credentials))
    }

    fn read(&mut self, account: &str) -> Result<Stored<String>, Error> {
        Ok(match self.load()?.entries.get(account) {
            Some(value) => Stored::Ready(value.clone()),
            None => Stored::Missing,
        })
    }

    fn save(&mut self, account: &str, value: Option<&str>) -> Result<(), Error> {
        let current = self.load()?;
        if current.entries.get(account).map(String::as_str) == value {
            return Ok(());
        }
        let mut next = current.clone();
        match value {
            Some(value) => {
                next.entries.insert(account.to_owned(), value.to_owned());
            }
            None => {
                next.entries.remove(account);
            }
        }
        self.backend.save(&serde_json::to_string(&next)?)?;
        self.cache = Some(next);
        Ok(())
    }
}

pub(super) fn read(account: &str) -> Result<Stored<String>, Error> {
    STORE
        .lock()
        .map_err(|_| Error::StoreSynchronization)?
        .read(account)
}

pub(super) fn save(account: &str, value: Option<&str>) -> Result<(), Error> {
    STORE
        .lock()
        .map_err(|_| Error::StoreSynchronization)?
        .save(account, value)
}

#[cfg(test)]
#[path = "../../tests/unit/store.rs"]
mod tests;
