use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use signoz_client::{ClientError, Credentials, ErrorCode, SigNozOrigin};

use crate::args::CredentialBackend;

/// `keychain`, `security`, or `file`; overrides the config file's `credential_store` setting.
pub const CREDENTIAL_STORE_ENV: &str = "SIGNOZ_CLI_CREDENTIAL_STORE";
/// The keyring service that names every stored API key.
const SERVICE: &str = "signoz-cli";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileConfig {
    pub url: String,
    /// What the environment is for, so people and agents can choose the right profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub current_profile: Option<String>,
    /// Where API keys are kept; the keychain when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_store: Option<CredentialBackend>,
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileConfig>,
}

pub trait CredentialStore {
    fn get(&self, name: &str) -> Result<Option<String>, String>;
    fn set(&self, name: &str, value: &str) -> Result<(), String>;
    fn delete(&self, name: &str) -> Result<(), String>;
}

impl<T: CredentialStore + ?Sized> CredentialStore for Box<T> {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        (**self).get(name)
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        (**self).set(name, value)
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        (**self).delete(name)
    }
}

/// The operating-system keyring. On macOS each CLI build is approved separately.
pub struct SystemKeyring;

impl CredentialStore for SystemKeyring {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        match keyring::Entry::new(SERVICE, name)
            .map_err(|error| error.to_string())?
            .get_password()
        {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, name)
            .map_err(|error| error.to_string())?
            .set_password(value)
            .map_err(|error| error.to_string())
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(SERVICE, name).map_err(|error| error.to_string())?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// macOS `/usr/bin/security`: the same login keychain entries as [`SystemKeyring`], approved once
/// for that Apple-signed tool instead of for every CLI build.
pub struct SecurityCli;

/// The exit status `security` returns for an entry that does not exist.
const SECURITY_NOT_FOUND: i32 = 44;

impl SecurityCli {
    fn run(arguments: &[&str], input: Option<&str>) -> Result<std::process::Output, String> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new("/usr/bin/security")
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("cannot run /usr/bin/security: {error}"))?;
        let mut stdin = child.stdin.take().expect("stdin is piped");
        if let Some(input) = input {
            // A failed write surfaces as the command's own failure.
            let _ = stdin.write_all(input.as_bytes());
        }
        drop(stdin);
        child
            .wait_with_output()
            .map_err(|error| format!("cannot run /usr/bin/security: {error}"))
    }

    fn failure(action: &str, output: &std::process::Output) -> String {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let detail = if detail.is_empty() {
            format!("exit status {:?}", output.status.code())
        } else {
            detail
        };
        format!("the macOS keychain could not {action} the API key: {detail}")
    }
}

impl CredentialStore for SecurityCli {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        let output = Self::run(
            &["find-generic-password", "-s", SERVICE, "-a", name, "-w"],
            None,
        )?;
        match output.status.code() {
            Some(0) => {
                let value = String::from_utf8(output.stdout)
                    .map_err(|_| "the stored API key is not valid UTF-8".to_owned())?;
                let value = value.strip_suffix('\n').unwrap_or(&value);
                Ok((!value.is_empty()).then(|| value.to_owned()))
            }
            Some(SECURITY_NOT_FOUND) => Ok(None),
            _ => Err(Self::failure("read", &output)),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        if value.contains(['\n', '\r']) {
            return Err("API keys must not contain line breaks".into());
        }
        // `-w` without a value reads the secret twice from stdin, keeping it out of the process list.
        let output = Self::run(
            &[
                "add-generic-password",
                "-U",
                "-s",
                SERVICE,
                "-a",
                name,
                "-w",
            ],
            Some(&format!("{value}\n{value}\n")),
        )?;
        if output.status.success() {
            Ok(())
        } else {
            Err(Self::failure("store", &output))
        }
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        let output = Self::run(
            &["delete-generic-password", "-s", SERVICE, "-a", name],
            None,
        )?;
        match output.status.code() {
            Some(0 | SECURITY_NOT_FOUND) => Ok(()),
            _ => Err(Self::failure("remove", &output)),
        }
    }
}

/// API keys in `credentials.toml` beside the config file, readable only by its owner. Nothing
/// ever prompts, but the values are stored in plain text.
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    pub fn beside(config_path: &Path) -> Self {
        Self {
            path: config_path.with_file_name("credentials.toml"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Result<BTreeMap<String, String>, String> {
        match fs::read_to_string(&self.path) {
            Ok(text) => toml::from_str(&text).map_err(|error| {
                format!("invalid credentials file {}: {error}", self.path.display())
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(error) => Err(format!("cannot read {}: {error}", self.path.display())),
        }
    }

    fn write(&self, credentials: &BTreeMap<String, String>) -> Result<(), String> {
        use std::io::Write;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let text = toml::to_string(credentials).map_err(|error| error.to_string())?;
        let temporary = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        let _ = fs::remove_file(&temporary);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let written = options
            .open(&temporary)
            .and_then(|mut file| file.write_all(text.as_bytes()))
            .and_then(|()| fs::rename(&temporary, &self.path));
        written.map_err(|error| {
            let _ = fs::remove_file(&temporary);
            format!("cannot write {}: {error}", self.path.display())
        })
    }
}

impl CredentialStore for FileStore {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        Ok(self.read()?.remove(name))
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        let mut credentials = self.read()?;
        credentials.insert(name.to_owned(), value.to_owned());
        self.write(&credentials)
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        let mut credentials = self.read()?;
        if credentials.remove(name).is_some() {
            self.write(&credentials)?;
        }
        Ok(())
    }
}

/// `SIGNOZ_CLI_CREDENTIAL_STORE`, if set; it takes precedence over the config file.
pub fn credential_store_override() -> Result<Option<CredentialBackend>, String> {
    parse_credential_store(
        std::env::var(CREDENTIAL_STORE_ENV)
            .ok()
            .filter(|value| !value.is_empty())
            .as_deref(),
    )
}

fn parse_credential_store(value: Option<&str>) -> Result<Option<CredentialBackend>, String> {
    value
        .map(|value| {
            CredentialBackend::from_str(value, true).map_err(|_| {
                format!("{CREDENTIAL_STORE_ENV} must be keychain, security, or file, not '{value}'")
            })
        })
        .transpose()
}

/// Open a credential store; `security` exists only on macOS.
pub fn open_credential_store(
    backend: CredentialBackend,
    config_path: &Path,
) -> Result<Box<dyn CredentialStore>, String> {
    match backend {
        CredentialBackend::Keychain => Ok(Box::new(SystemKeyring)),
        CredentialBackend::Security if cfg!(target_os = "macos") => Ok(Box::new(SecurityCli)),
        CredentialBackend::Security => {
            Err("the security credential store is available only on macOS".into())
        }
        CredentialBackend::File => Ok(Box::new(FileStore::beside(config_path))),
    }
}

/// Where a backend keeps API keys, as shown to people.
pub fn credential_store_label(backend: CredentialBackend) -> &'static str {
    match backend {
        CredentialBackend::Keychain => "the operating-system keychain",
        CredentialBackend::Security => "the macOS keychain through /usr/bin/security",
        CredentialBackend::File => "the credentials file",
    }
}

/// What moving API keys to another store did.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Migration {
    /// API keys copied to the new store.
    pub moved: usize,
    /// Profiles whose API key the previous store did not hold.
    pub missing: usize,
    /// Entries to remove from the previous store once the config file is saved.
    #[serde(skip)]
    pub remove_from_previous: Vec<String>,
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
                description: None,
            },
        );
        Ok(())
    }

    /// Clear the old credential before changing its destination. Returns whether the URL changed.
    pub fn update_profile(
        &mut self,
        name: &str,
        url: &str,
        store: &dyn CredentialStore,
    ) -> Result<bool, ClientError> {
        validate_profile_name(name)
            .map_err(|error| ClientError::new(ErrorCode::Validation, error))?;
        let origin = SigNozOrigin::parse(url)
            .map_err(|error| ClientError::new(ErrorCode::Validation, error))?;
        let profile = self.profiles.get_mut(name).ok_or_else(|| {
            ClientError::new(
                ErrorCode::Validation,
                format!("profile '{name}' does not exist"),
            )
        })?;
        if SigNozOrigin::parse(&profile.url).ok().as_ref() == Some(&origin) {
            return Ok(false);
        }
        store.delete(name).map_err(|error| {
            ClientError::new(
                ErrorCode::Config,
                format!("could not clear the old API key; profile URL was not changed: {error}"),
            )
        })?;
        profile.url = origin.to_string();
        Ok(true)
    }

    /// Used by onboarding, which replaces the credential and rolls it back if saving fails.
    pub(crate) fn upsert_profile(&mut self, name: &str, url: &str) -> Result<(), String> {
        validate_profile_name(name)?;
        let url = SigNozOrigin::parse(url)?.to_string();
        match self.profiles.get_mut(name) {
            // Re-running setup keeps the profile's description.
            Some(profile) => profile.url = url,
            None => {
                self.profiles.insert(
                    name.to_owned(),
                    ProfileConfig {
                        url,
                        description: None,
                    },
                );
            }
        }
        Ok(())
    }

    /// `SIGNOZ_CLI_CREDENTIAL_STORE`, then the config file, then the keychain.
    pub fn credential_backend(&self) -> Result<CredentialBackend, String> {
        Ok(credential_store_override()?
            .or(self.credential_store)
            .unwrap_or_default())
    }

    /// Copy every profile's API key from `from` to `to`. Nothing is removed here: the caller saves
    /// the config file, then removes [`Migration::remove_from_previous`], so a failure leaves the
    /// previous store intact.
    pub fn migrate_credentials(
        &self,
        from: &dyn CredentialStore,
        to: &dyn CredentialStore,
    ) -> Result<Migration, String> {
        let mut migration = Migration::default();
        let mut copies = Vec::new();
        for name in self.profiles.keys() {
            match from.get(name)? {
                Some(key) => copies.push((name.as_str(), key)),
                None => migration.missing += 1,
            }
        }
        let mut written: Vec<&str> = Vec::new();
        for (name, key) in &copies {
            if let Err(error) = to.set(name, key) {
                for name in &written {
                    let _ = to.delete(name);
                }
                return Err(format!(
                    "could not copy the API key of '{name}' to the new store, so nothing was moved: {error}"
                ));
            }
            written.push(name);
        }
        migration.moved = written.len();
        migration.remove_from_previous = written.into_iter().map(str::to_owned).collect();
        Ok(migration)
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
    fn changing_a_profile_url_requires_a_new_key_and_preserves_other_profiles() {
        for new_url in [
            "https://other.example.com",
            "http://signoz.example.com",
            "https://signoz.example.com/another-instance",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("config.toml");
            let store = MemoryStore::default();
            let mut config = ConfigFile::default();
            config
                .add_profile("prod", "https://signoz.example.com")
                .unwrap();
            config
                .add_profile("test", "https://test.example.com")
                .unwrap();
            config.use_profile("prod").unwrap();
            store.set("prod", "old-key").unwrap();
            store.set("test", "test-key").unwrap();

            assert!(config.update_profile("prod", new_url, &store).unwrap());
            config.save(&path).unwrap();
            let config = ConfigFile::load(&path).unwrap();
            assert_eq!(config.profiles["prod"].url, new_url);
            assert_eq!(config.current_profile.as_deref(), Some("prod"));
            assert_eq!(store.get("prod").unwrap(), None);
            assert_eq!(store.get("test").unwrap().as_deref(), Some("test-key"));
            let error = config
                .resolve_inputs(None, Some("prod"), None, None, &store)
                .unwrap_err();
            assert!(error.contains("auth login"));

            store.set("prod", "new-key").unwrap();
            let connection = config
                .resolve_inputs(None, Some("prod"), None, None, &store)
                .unwrap();
            assert_eq!(connection.origin.as_str(), new_url);
        }
    }

    #[test]
    fn an_equivalent_or_invalid_url_never_clears_the_key() {
        let store = MemoryStore::default();
        let mut config = ConfigFile::default();
        config
            .add_profile("prod", "https://signoz.example.com")
            .unwrap();
        store.set("prod", "old-key").unwrap();
        assert!(
            !config
                .update_profile("prod", "https://SIGNOZ.example.com:443/", &store)
                .unwrap()
        );
        assert_eq!(
            config
                .update_profile("prod", "file:///tmp/signoz", &store)
                .unwrap_err()
                .code(),
            ErrorCode::Validation
        );
        assert_eq!(config.profiles["prod"].url, "https://signoz.example.com");
        assert_eq!(store.get("prod").unwrap().as_deref(), Some("old-key"));
    }

    #[test]
    fn failure_to_clear_a_key_prevents_changing_its_destination() {
        struct UnavailableStore;
        impl CredentialStore for UnavailableStore {
            fn get(&self, _: &str) -> Result<Option<String>, String> {
                panic!("must not read keys")
            }
            fn set(&self, _: &str, _: &str) -> Result<(), String> {
                panic!("must not write keys")
            }
            fn delete(&self, _: &str) -> Result<(), String> {
                Err("keyring unavailable".into())
            }
        }
        let mut config = ConfigFile::default();
        config
            .add_profile("prod", "https://original.example.com")
            .unwrap();
        assert!(
            !config
                .update_profile("prod", "https://original.example.com/", &UnavailableStore)
                .unwrap()
        );
        let error = config
            .update_profile("prod", "https://new.example.com", &UnavailableStore)
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::Config);
        assert_eq!(config.profiles["prod"].url, "https://original.example.com");
        assert_eq!(
            config
                .update_profile("missing", "https://new.example.com", &UnavailableStore)
                .unwrap_err()
                .code(),
            ErrorCode::Validation
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

        config.profiles.get_mut("prod").unwrap().description = Some("Production".into());
        config
            .upsert_profile("prod", "https://second.signoz.example.com")
            .unwrap();
        assert_eq!(
            config.profiles["prod"].url,
            "https://second.signoz.example.com"
        );
        assert_eq!(
            config.profiles["prod"].description.as_deref(),
            Some("Production")
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

    #[test]
    fn file_store_keeps_api_keys_readable_only_by_their_owner() {
        let directory = tempfile::tempdir().unwrap();
        let store = FileStore::beside(&directory.path().join("config.toml"));
        assert_eq!(store.get("prod").unwrap(), None);
        store.set("prod", "prod-key").unwrap();
        store.set("test", "test-key").unwrap();
        assert_eq!(store.get("prod").unwrap().as_deref(), Some("prod-key"));
        store.delete("prod").unwrap();
        assert_eq!(store.get("prod").unwrap(), None);
        assert_eq!(store.get("test").unwrap().as_deref(), Some("test-key"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(store.path()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn credential_store_settings_parse_and_default_to_the_keychain() {
        assert_eq!(parse_credential_store(None), Ok(None));
        assert_eq!(
            parse_credential_store(Some("security")),
            Ok(Some(CredentialBackend::Security))
        );
        assert_eq!(
            parse_credential_store(Some("FILE")),
            Ok(Some(CredentialBackend::File))
        );
        assert!(
            parse_credential_store(Some("vault"))
                .unwrap_err()
                .contains(CREDENTIAL_STORE_ENV)
        );
        assert_eq!(
            ConfigFile::default().credential_store.unwrap_or_default(),
            CredentialBackend::Keychain
        );
    }

    #[test]
    fn migration_copies_api_keys_and_undoes_a_failed_copy() {
        let mut config = ConfigFile::default();
        for name in ["prod", "stale", "test"] {
            config
                .add_profile(name, "https://signoz.example.com")
                .unwrap();
        }
        let from = MemoryStore::default();
        from.set("prod", "prod-key").unwrap();
        from.set("test", "test-key").unwrap();
        let to = MemoryStore::default();
        let migration = config.migrate_credentials(&from, &to).unwrap();
        assert_eq!((migration.moved, migration.missing), (2, 1));
        assert_eq!(migration.remove_from_previous, ["prod", "test"]);
        assert_eq!(to.get("test").unwrap().as_deref(), Some("test-key"));
        // The caller removes the old copies only after saving the config file.
        assert_eq!(from.get("prod").unwrap().as_deref(), Some("prod-key"));

        /// Accepts the first write and refuses the rest.
        struct FlakyStore(MemoryStore, Mutex<bool>);
        impl CredentialStore for FlakyStore {
            fn get(&self, name: &str) -> Result<Option<String>, String> {
                self.0.get(name)
            }
            fn set(&self, name: &str, value: &str) -> Result<(), String> {
                let mut used = self.1.lock().unwrap();
                if std::mem::replace(&mut *used, true) {
                    return Err("disk full".into());
                }
                self.0.set(name, value)
            }
            fn delete(&self, name: &str) -> Result<(), String> {
                self.0.delete(name)
            }
        }
        let flaky = FlakyStore(MemoryStore::default(), Mutex::new(false));
        let error = config.migrate_credentials(&from, &flaky).unwrap_err();
        assert!(error.contains("nothing was moved"), "{error}");
        assert_eq!(flaky.get("prod").unwrap(), None);
    }
}
