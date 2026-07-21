use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use signoz_client::{Credentials, SigNozOrigin};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileConfig {
    pub url: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub current_profile: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileConfig>,
}

pub trait CredentialStore {
    fn get(&self, name: &str) -> Result<Option<String>, String>;
    fn set(&self, name: &str, value: &str) -> Result<(), String>;
    fn delete(&self, name: &str) -> Result<(), String>;
}

pub struct SystemKeyring;

impl CredentialStore for SystemKeyring {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        match keyring::Entry::new("signoz-cli", name)
            .map_err(|error| error.to_string())?
            .get_password()
        {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new("signoz-cli", name)
            .map_err(|error| error.to_string())?
            .set_password(value)
            .map_err(|error| error.to_string())
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        let entry = keyring::Entry::new("signoz-cli", name).map_err(|error| error.to_string())?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

#[derive(Debug)]
pub struct ResolvedConnection {
    pub origin: SigNozOrigin,
    pub credentials: Credentials,
    pub profile: Option<String>,
}

impl ConfigFile {
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
        toml::from_str(&text).map_err(|error| format!("invalid config: {error}"))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let text = toml::to_string_pretty(self).map_err(|error| error.to_string())?;
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        fs::write(&temporary, text).map_err(|error| error.to_string())?;
        fs::rename(&temporary, path).map_err(|error| error.to_string())
    }

    pub fn add_profile(&mut self, name: &str, url: &str) -> Result<(), String> {
        validate_profile_name(name)?;
        let origin = SigNozOrigin::parse(url)?;
        self.profiles.insert(
            name.to_owned(),
            ProfileConfig {
                url: origin.to_string(),
            },
        );
        Ok(())
    }

    pub fn use_profile(&mut self, name: &str) -> Result<(), String> {
        if !self.profiles.contains_key(name) {
            return Err(format!("profile '{name}' does not exist"));
        }
        self.current_profile = Some(name.to_owned());
        Ok(())
    }

    pub fn remove_profile(
        &mut self,
        name: &str,
        store: &dyn CredentialStore,
    ) -> Result<(), String> {
        if self.profiles.remove(name).is_none() {
            return Err(format!("profile '{name}' does not exist"));
        }
        store.delete(name)?;
        if self.current_profile.as_deref() == Some(name) {
            self.current_profile = None;
        }
        Ok(())
    }

    pub fn resolve(
        &self,
        explicit_url: Option<&str>,
        selected_profile: Option<&str>,
        environment_key: Option<&str>,
        store: &dyn CredentialStore,
    ) -> Result<ResolvedConnection, String> {
        let env_url = std::env::var("SIGNOZ_URL").ok();
        if let Some(url) = explicit_url.or(env_url.as_deref()) {
            let key = environment_key
                .map(str::to_owned)
                .or_else(|| std::env::var("SIGNOZ_API_KEY").ok())
                .filter(|key| !key.is_empty())
                .ok_or_else(|| "SIGNOZ_API_KEY is required with --url or SIGNOZ_URL".to_owned())?;
            return Ok(ResolvedConnection {
                origin: SigNozOrigin::parse(url)?,
                credentials: Credentials::new(key),
                profile: None,
            });
        }
        let profile_name = selected_profile
            .map(str::to_owned)
            .or_else(|| self.current_profile.clone())
            .ok_or_else(|| "no connection: use --url with SIGNOZ_API_KEY or select a profile".to_owned())?;
        let profile = self
            .profiles
            .get(&profile_name)
            .ok_or_else(|| format!("profile '{profile_name}' does not exist"))?;
        let key = environment_key
            .map(str::to_owned)
            .or_else(|| std::env::var("SIGNOZ_API_KEY").ok())
            .or(store.get(&profile_name)?)
            .filter(|key| !key.is_empty())
            .ok_or_else(|| format!("profile '{profile_name}' has no API key; run auth login"))?;
        Ok(ResolvedConnection {
            origin: SigNozOrigin::parse(&profile.url)?,
            credentials: Credentials::new(key),
            profile: Some(profile_name),
        })
    }
}

pub fn default_config_path() -> Result<PathBuf, String> {
    ProjectDirs::from("dev", "signoz", "signoz-cli")
        .map(|directories| directories.config_dir().join("config.toml"))
        .ok_or_else(|| "unable to determine the operating-system config directory".to_owned())
}

fn validate_profile_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("profile names may contain only letters, numbers, '-' and '_'".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryStore(Mutex<HashMap<String, String>>);

    impl CredentialStore for MemoryStore {
        fn get(&self, name: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(name).cloned())
        }
        fn set(&self, name: &str, value: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(name.into(), value.into());
            Ok(())
        }
        fn delete(&self, name: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(name);
            Ok(())
        }
    }

    #[test]
    fn persists_profiles_without_secrets_and_resolves_selected_profile() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let store = MemoryStore::default();
        let mut config = ConfigFile::default();
        config.add_profile("prod", "https://signoz.example.com").unwrap();
        config.current_profile = Some("prod".into());
        store.set("prod", "never-serialize-this").unwrap();
        config.save(&path).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("never-serialize-this"));
        let resolved = config.resolve(None, None, None, &store).unwrap();
        assert_eq!(resolved.origin.as_str(), "https://signoz.example.com");
        assert!(!format!("{:?}", resolved.credentials).contains("never-serialize-this"));
    }

    #[test]
    fn removing_a_profile_deletes_its_credential() {
        let store = MemoryStore::default();
        let mut config = ConfigFile::default();
        config.add_profile("dev", "http://localhost:3301").unwrap();
        store.set("dev", "secret").unwrap();
        config.remove_profile("dev", &store).unwrap();
        assert_eq!(store.get("dev").unwrap(), None);
    }
}
