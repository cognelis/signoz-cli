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

#[derive(Debug)]
pub struct ResolvedTarget {
    pub origin: SigNozOrigin,
    pub profile: Option<String>,
    credential_source: CredentialSource,
}

#[derive(Debug)]
enum CredentialSource {
    Environment { missing_message: &'static str },
    Profile,
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
        if self.profiles.contains_key(name) {
            return Err(format!(
                "profile '{name}' already exists; remove it before using a different URL"
            ));
        }
        let origin = SigNozOrigin::parse(url)?;
        self.profiles.insert(
            name.to_owned(),
            ProfileConfig {
                url: origin.to_string(),
            },
        );
        Ok(())
    }

    pub fn update_profile(&mut self, name: &str, url: &str) -> Result<(), String> {
        validate_profile_name(name)?;
        if !self.profiles.contains_key(name) {
            return Err(format!("profile '{name}' does not exist"));
        }
        let origin = SigNozOrigin::parse(url)?;
        self.profiles.insert(
            name.to_owned(),
            ProfileConfig {
                url: origin.to_string(),
            },
        );
        Ok(())
    }

    pub fn upsert_profile(&mut self, name: &str, url: &str) -> Result<(), String> {
        if self.profiles.contains_key(name) {
            self.update_profile(name, url)
        } else {
            self.add_profile(name, url)
        }
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
        let environment_url = std::env::var("SIGNOZ_URL").ok();
        self.resolve_inputs(
            explicit_url,
            selected_profile,
            environment_url.as_deref(),
            environment_key,
            store,
        )
    }

    pub fn resolve_target(
        &self,
        explicit_url: Option<&str>,
        selected_profile: Option<&str>,
    ) -> Result<ResolvedTarget, String> {
        let environment_url = std::env::var("SIGNOZ_URL").ok();
        self.resolve_target_inputs(explicit_url, selected_profile, environment_url.as_deref())
    }

    pub fn resolve_credentials(
        &self,
        target: ResolvedTarget,
        environment_key: Option<&str>,
        store: &dyn CredentialStore,
    ) -> Result<ResolvedConnection, String> {
        let key = match target.credential_source {
            CredentialSource::Environment { missing_message } => environment_key
                .map(str::to_owned)
                .filter(|key| !key.is_empty())
                .ok_or_else(|| missing_message.to_owned())?,
            CredentialSource::Profile => {
                let profile = target
                    .profile
                    .as_deref()
                    .expect("profile credential targets always carry a profile name");
                if let Some(key) = environment_key {
                    Some(key.to_owned())
                } else {
                    store.get(profile)?
                }
                .filter(|key| !key.is_empty())
                .ok_or_else(|| format!("profile '{profile}' has no API key; run auth login"))?
            }
        };
        Ok(ResolvedConnection {
            origin: target.origin,
            credentials: Credentials::new(key),
            profile: target.profile,
        })
    }

    fn resolve_inputs(
        &self,
        explicit_url: Option<&str>,
        selected_profile: Option<&str>,
        environment_url: Option<&str>,
        environment_key: Option<&str>,
        store: &dyn CredentialStore,
    ) -> Result<ResolvedConnection, String> {
        let target = self.resolve_target_inputs(explicit_url, selected_profile, environment_url)?;
        self.resolve_credentials(target, environment_key, store)
    }

    fn resolve_target_inputs(
        &self,
        explicit_url: Option<&str>,
        selected_profile: Option<&str>,
        environment_url: Option<&str>,
    ) -> Result<ResolvedTarget, String> {
        if let Some(url) = explicit_url {
            return Ok(ResolvedTarget {
                origin: SigNozOrigin::parse(url)?,
                profile: None,
                credential_source: CredentialSource::Environment {
                    missing_message: "SIGNOZ_API_KEY is required with --url or SIGNOZ_URL",
                },
            });
        }
        if let Some(profile_name) = selected_profile {
            return self.resolve_profile_target(profile_name);
        }
        if let Some(url) = environment_url {
            return Ok(ResolvedTarget {
                origin: SigNozOrigin::parse(url)?,
                profile: None,
                credential_source: CredentialSource::Environment {
                    missing_message: "SIGNOZ_API_KEY is required with SIGNOZ_URL",
                },
            });
        }
        let profile_name = self.current_profile.as_deref().ok_or_else(|| {
            "no connection: run `signoz-cli init`, or use --url with SIGNOZ_API_KEY".to_owned()
        })?;
        self.resolve_profile_target(profile_name)
    }

    fn resolve_profile_target(&self, profile_name: &str) -> Result<ResolvedTarget, String> {
        let profile = self
            .profiles
            .get(profile_name)
            .ok_or_else(|| format!("profile '{profile_name}' does not exist"))?;
        Ok(ResolvedTarget {
            origin: SigNozOrigin::parse(&profile.url)?,
            profile: Some(profile_name.to_owned()),
            credential_source: CredentialSource::Profile,
        })
    }
}

pub fn default_config_path() -> Result<PathBuf, String> {
    ProjectDirs::from("dev", "signoz", "signoz-cli")
        .map(|directories| directories.config_dir().join("config.toml"))
        .ok_or_else(|| "unable to determine the operating-system config directory".to_owned())
}

pub(crate) fn validate_profile_name(name: &str) -> Result<(), String> {
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
        config
            .add_profile("prod", "https://signoz.example.com")
            .unwrap();
        config.current_profile = Some("prod".into());
        store.set("prod", "never-serialize-this").unwrap();
        config.save(&path).unwrap();
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("never-serialize-this")
        );
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

    #[test]
    fn adding_an_existing_profile_never_rebinds_its_credential_to_a_new_origin() {
        let store = MemoryStore::default();
        let mut config = ConfigFile::default();
        config
            .add_profile("prod", "https://original.signoz.example.com")
            .unwrap();
        store.set("prod", "original-secret").unwrap();
        let error = config
            .add_profile("prod", "https://attacker.invalid")
            .unwrap_err();
        assert!(error.contains("already exists"));
        assert_eq!(
            config.profiles["prod"].url,
            "https://original.signoz.example.com"
        );
        assert_eq!(
            store.get("prod").unwrap().as_deref(),
            Some("original-secret")
        );
    }

    #[test]
    fn upsert_profile_adds_and_then_updates_a_validated_origin() {
        let mut config = ConfigFile::default();
        config
            .upsert_profile("prod", "https://first.signoz.example.com/")
            .unwrap();
        assert_eq!(
            config.profiles["prod"].url,
            "https://first.signoz.example.com"
        );

        config
            .upsert_profile("prod", "https://second.signoz.example.com")
            .unwrap();
        assert_eq!(
            config.profiles["prod"].url,
            "https://second.signoz.example.com"
        );
    }

    #[test]
    fn environment_key_does_not_touch_the_keyring() {
        struct FailingStore;
        impl CredentialStore for FailingStore {
            fn get(&self, _: &str) -> Result<Option<String>, String> {
                Err("keyring unavailable".into())
            }
            fn set(&self, _: &str, _: &str) -> Result<(), String> {
                unreachable!()
            }
            fn delete(&self, _: &str) -> Result<(), String> {
                unreachable!()
            }
        }
        let mut config = ConfigFile::default();
        config
            .add_profile("ci", "https://signoz.example.com")
            .unwrap();
        let resolved = config
            .resolve(None, Some("ci"), Some("environment-secret"), &FailingStore)
            .unwrap();
        assert_eq!(resolved.profile.as_deref(), Some("ci"));
    }

    #[test]
    fn target_selection_needs_no_key_and_preserves_connection_precedence() {
        let mut config = ConfigFile::default();
        config
            .add_profile("selected", "https://selected.example.com")
            .unwrap();
        config
            .add_profile("current", "https://current.example.com")
            .unwrap();
        config.current_profile = Some("current".into());

        let explicit = config
            .resolve_target_inputs(
                Some("https://explicit.example.com"),
                Some("selected"),
                Some("https://environment.example.com"),
            )
            .unwrap();
        assert_eq!(explicit.origin.as_str(), "https://explicit.example.com");
        assert_eq!(explicit.profile, None);

        let selected = config
            .resolve_target_inputs(
                None,
                Some("selected"),
                Some("https://environment.example.com"),
            )
            .unwrap();
        assert_eq!(selected.origin.as_str(), "https://selected.example.com");
        assert_eq!(selected.profile.as_deref(), Some("selected"));

        let environment = config
            .resolve_target_inputs(None, None, Some("https://environment.example.com"))
            .unwrap();
        assert_eq!(
            environment.origin.as_str(),
            "https://environment.example.com"
        );
        assert_eq!(environment.profile, None);

        let current = config.resolve_target_inputs(None, None, None).unwrap();
        assert_eq!(current.origin.as_str(), "https://current.example.com");
        assert_eq!(current.profile.as_deref(), Some("current"));
    }

    #[test]
    fn credential_resolution_happens_after_target_selection() {
        let store = MemoryStore::default();
        let mut config = ConfigFile::default();
        config
            .add_profile("prod", "https://signoz.example.com")
            .unwrap();
        store.set("prod", "keyring-secret").unwrap();

        let target = config
            .resolve_target_inputs(None, Some("prod"), None)
            .unwrap();
        let resolved = config.resolve_credentials(target, None, &store).unwrap();
        assert_eq!(resolved.profile.as_deref(), Some("prod"));
        assert!(!format!("{:?}", resolved.credentials).contains("keyring-secret"));

        let target = config
            .resolve_target_inputs(Some("https://direct.example.com"), None, None)
            .unwrap();
        let error = config
            .resolve_credentials(target, None, &store)
            .unwrap_err();
        assert!(error.contains("SIGNOZ_API_KEY"));
    }

    #[test]
    fn explicit_profile_wins_over_environment_url() {
        let store = MemoryStore::default();
        let mut config = ConfigFile::default();
        config
            .add_profile("prod", "https://profile.signoz.example.com")
            .unwrap();
        let resolved = config
            .resolve_inputs(
                None,
                Some("prod"),
                Some("https://environment.signoz.example.com"),
                Some("environment-secret"),
                &store,
            )
            .unwrap();
        assert_eq!(
            resolved.origin.as_str(),
            "https://profile.signoz.example.com"
        );
        assert_eq!(resolved.profile.as_deref(), Some("prod"));
    }

    #[test]
    fn explicit_url_wins_over_profile_and_environment_url() {
        let store = MemoryStore::default();
        let mut config = ConfigFile::default();
        config
            .add_profile("prod", "https://profile.signoz.example.com")
            .unwrap();
        let resolved = config
            .resolve_inputs(
                Some("https://explicit.signoz.example.com"),
                Some("prod"),
                Some("https://environment.signoz.example.com"),
                Some("environment-secret"),
                &store,
            )
            .unwrap();
        assert_eq!(
            resolved.origin.as_str(),
            "https://explicit.signoz.example.com"
        );
        assert_eq!(resolved.profile, None);
    }
}
