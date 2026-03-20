use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, Result};

pub trait SecretStoreBackend: Send + Sync {
    fn backend_name(&self) -> &'static str;
    fn set_secret(&self, key: &str, value: &str) -> Result<()>;
    fn get_secret(&self, key: &str) -> Result<Option<String>>;
    fn delete_secret(&self, key: &str) -> Result<()>;
}

#[derive(Clone)]
pub struct SecretManager {
    backend: Arc<dyn SecretStoreBackend>,
}

impl SecretManager {
    pub fn new(backend: Arc<dyn SecretStoreBackend>) -> Self {
        Self { backend }
    }

    pub fn new_memory() -> Self {
        Self::new(Arc::new(MemorySecretStore::default()))
    }

    pub fn new_keyring(service_name: impl Into<String>) -> Self {
        Self::new(Arc::new(KeyringSecretStore::new(service_name)))
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.backend_name()
    }

    pub fn set_secret(&self, key: &str, value: &str) -> Result<()> {
        self.backend.set_secret(key, value)
    }

    pub fn get_secret(&self, key: &str) -> Result<Option<String>> {
        self.backend.get_secret(key)
    }

    pub fn delete_secret(&self, key: &str) -> Result<()> {
        self.backend.delete_secret(key)
    }
}

#[derive(Default)]
pub struct MemorySecretStore {
    values: Mutex<HashMap<String, String>>,
}

impl SecretStoreBackend for MemorySecretStore {
    fn backend_name(&self) -> &'static str {
        "memory"
    }

    fn set_secret(&self, key: &str, value: &str) -> Result<()> {
        self.values
            .lock()
            .map_err(|_| anyhow!("memory secret store is poisoned"))?
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn get_secret(&self, key: &str) -> Result<Option<String>> {
        let value = self
            .values
            .lock()
            .map_err(|_| anyhow!("memory secret store is poisoned"))?
            .get(key)
            .cloned();
        Ok(value)
    }

    fn delete_secret(&self, key: &str) -> Result<()> {
        self.values
            .lock()
            .map_err(|_| anyhow!("memory secret store is poisoned"))?
            .remove(key);
        Ok(())
    }
}

pub struct KeyringSecretStore {
    service_name: String,
}

impl KeyringSecretStore {
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
        }
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service_name, key).map_err(Into::into)
    }
}

impl SecretStoreBackend for KeyringSecretStore {
    fn backend_name(&self) -> &'static str {
        "keyring"
    }

    fn set_secret(&self, key: &str, value: &str) -> Result<()> {
        self.entry(key)?.set_password(value)?;
        Ok(())
    }

    fn get_secret(&self, key: &str) -> Result<Option<String>> {
        match self.entry(key)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn delete_secret(&self, key: &str) -> Result<()> {
        match self.entry(key)?.delete_credential() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SecretManager;

    #[test]
    fn memory_secret_store_round_trips_values() {
        let manager = SecretManager::new_memory();

        manager
            .set_secret("anthropic_api_key", "test-secret")
            .expect("set secret");
        assert_eq!(
            manager
                .get_secret("anthropic_api_key")
                .expect("get secret")
                .as_deref(),
            Some("test-secret")
        );

        manager
            .delete_secret("anthropic_api_key")
            .expect("delete secret");
        assert_eq!(
            manager.get_secret("anthropic_api_key").expect("get secret"),
            None
        );
    }
}
