use std::io::{self, IsTerminal, Read};
use std::path::Path;
use std::time::{Duration, Instant};

use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use serde_json::{Value, json};
use signoz_client::{Credentials, Operation, SigNozClient, SigNozOrigin};

use crate::args::{OutputFormat, SkillTarget};
use crate::config::{ConfigFile, CredentialStore, validate_profile_name};
use crate::dispatch::{AppError, DispatchResult, project_auth_identity};
use crate::doctor::DoctorReport;
use crate::output::Meta;
use crate::skill_install::{self, InstallResult};

const NON_INTERACTIVE_HINT: &str = "For automation, run `signoz-cli profile add <name> --url <url>`, then pipe the key to `signoz-cli --profile <name> auth login --api-key-stdin`";

struct SetupDraft {
    profile: String,
    url: String,
    api_key: SecretString,
    skill_target: SkillChoice,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SkillChoice {
    All,
    Codex,
    Claude,
    Skip,
}

impl SkillChoice {
    fn label(self) -> &'static str {
        match self {
            Self::All => "Codex and Claude Code",
            Self::Codex => "Codex only",
            Self::Claude => "Claude Code only",
            Self::Skip => "Skip Skill installation",
        }
    }

    fn target(self) -> Option<SkillTarget> {
        match self {
            Self::All => Some(SkillTarget::All),
            Self::Codex => Some(SkillTarget::Codex),
            Self::Claude => Some(SkillTarget::Claude),
            Self::Skip => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProfileChoice {
    Create,
    Existing(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VerificationFailureAction {
    Retry,
    Edit,
    Cancel,
}

struct Verification {
    identity: Value,
    status: u16,
    request_id: Option<String>,
    elapsed_ms: u128,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InitResult {
    profile: String,
    url: String,
    verified: bool,
    skills: Vec<InstallResult>,
    doctor: Value,
}

pub async fn run_init(
    selected_profile: Option<&str>,
    explicit_url: Option<&str>,
    timeout_seconds: u64,
    config_path: &Path,
    store: &dyn CredentialStore,
) -> Result<DispatchResult, AppError> {
    if !interactive_terminal() {
        return Err(
            AppError::validation("`signoz-cli init` requires an interactive terminal")
                .with_hint(NON_INTERACTIVE_HINT),
        );
    }

    let mut config = ConfigFile::load(config_path).map_err(AppError::config)?;
    prompt_io(cliclack::intro("SigNoz CLI setup"))?;

    let mut draft = prompt_setup(&config, selected_profile, explicit_url)?;
    let verification = loop {
        let spinner = cliclack::spinner();
        spinner.start("Verifying SigNoz connection and API key");
        match verify_key(&draft.url, draft.api_key.expose_secret(), timeout_seconds).await {
            Ok(verification) => {
                spinner.stop("Connection and API key verified");
                break verification;
            }
            Err(error) => {
                spinner.error(format!("Verification failed: {}", error.message));
                let action = prompt_io(
                    cliclack::select("How would you like to continue?")
                        .item(
                            VerificationFailureAction::Retry,
                            "Retry verification",
                            "Use the same URL and API key",
                        )
                        .item(
                            VerificationFailureAction::Edit,
                            "Edit connection details",
                            "Re-enter profile, URL, and API key",
                        )
                        .item(
                            VerificationFailureAction::Cancel,
                            "Cancel setup",
                            "Do not save configuration",
                        )
                        .interact(),
                )?;
                match action {
                    VerificationFailureAction::Retry => {}
                    VerificationFailureAction::Edit => {
                        draft = prompt_setup(&config, Some(&draft.profile), Some(&draft.url))?;
                    }
                    VerificationFailureAction::Cancel => return cancelled("Setup cancelled"),
                }
            }
        }
    };

    let skills = match draft.skill_target.target() {
        Some(target) => {
            let spinner = cliclack::spinner();
            spinner.start("Installing SigNoz Skills for coding agents");
            match skill_install::install(target) {
                Ok(results) => {
                    spinner.stop("Coding-agent Skills installed");
                    results
                }
                Err(error) => {
                    spinner.error("Skill installation failed");
                    return Err(AppError::config(error));
                }
            }
        }
        None => Vec::new(),
    };

    persist_setup(&mut config, config_path, store, &draft)?;
    let mut doctor = DoctorReport::new(env!("CARGO_PKG_VERSION"));
    doctor.configuration_passed();
    doctor.credentials_passed();
    doctor.api_passed();
    doctor.authentication_passed();
    prompt_io(cliclack::outro(format!(
        "Ready — profile '{}' is active. Run `signoz-cli doctor` to diagnose it anytime.",
        draft.profile
    )))?;

    Ok(DispatchResult {
        command: "init".into(),
        data: serde_json::to_value(InitResult {
            profile: draft.profile,
            url: draft.url,
            verified: true,
            skills,
            doctor: doctor.into_value(),
        })
        .expect("setup results contain only serializable values"),
        meta: Meta {
            profile: config.current_profile,
            status: Some(verification.status),
            request_id: verification.request_id,
            elapsed_ms: Some(verification.elapsed_ms),
            ..Meta::default()
        },
        failure_code: None,
    })
}

pub async fn run_login(
    selected_profile: Option<&str>,
    api_key_stdin: bool,
    no_verify: bool,
    timeout_seconds: u64,
    output: OutputFormat,
    config: &ConfigFile,
    store: &dyn CredentialStore,
) -> Result<DispatchResult, AppError> {
    let interactive = interactive_terminal();
    let profile = select_login_profile(config, selected_profile, interactive)?;
    let profile_config = config
        .profiles
        .get(&profile)
        .ok_or_else(|| AppError::config(format!("profile '{profile}' does not exist")))?;

    let api_key = if api_key_stdin {
        if io::stdin().is_terminal() {
            return Err(AppError::validation(
                "--api-key-stdin expects a pipe or redirected standard input",
            )
            .with_hint("Pipe the key with `printf '%s\\n' \"$SIGNOZ_API_KEY\" | signoz-cli auth login --api-key-stdin`"));
        }
        SecretString::from(read_api_key_from_stdin()?)
    } else {
        if !interactive {
            return Err(AppError::validation(
                "interactive API-key entry requires an interactive terminal",
            )
            .with_hint("Pipe the key to `signoz-cli auth login --api-key-stdin`"));
        }
        SecretString::from(prompt_io(
            cliclack::password(format!("API key for profile '{profile}'"))
                .validate(|value: &String| {
                    (!value.trim().is_empty())
                        .then_some(())
                        .ok_or("API key is required")
                })
                .interact(),
        )?)
    };

    let verification = if no_verify {
        None
    } else if interactive && !api_key_stdin && !matches!(output, OutputFormat::Json) {
        let spinner = cliclack::spinner();
        spinner.start("Verifying API key");
        match verify_key(
            &profile_config.url,
            api_key.expose_secret(),
            timeout_seconds,
        )
        .await
        {
            Ok(verification) => {
                spinner.stop("API key verified");
                Some(verification)
            }
            Err(error) => {
                spinner.error("API-key verification failed");
                return Err(error.with_hint(
                    "Check the URL and API key, or use --no-verify only when offline verification is intentional",
                ));
            }
        }
    } else {
        Some(
            verify_key(
                &profile_config.url,
                api_key.expose_secret(),
                timeout_seconds,
            )
            .await
            .map_err(|error| {
                error.with_hint(
                    "Check the URL and API key, or use --no-verify only when offline verification is intentional",
                )
            })?,
        )
    };

    store
        .set(&profile, api_key.expose_secret())
        .map_err(AppError::config)?;

    let mut meta = Meta {
        profile: Some(profile.clone()),
        ..Meta::default()
    };
    if no_verify {
        meta.warnings.push(
            "API key was stored without verification because --no-verify was explicitly set".into(),
        );
    }
    let identity = verification.as_ref().map(|value| value.identity.clone());
    if let Some(verification) = &verification {
        meta.status = Some(verification.status);
        meta.request_id.clone_from(&verification.request_id);
        meta.elapsed_ms = Some(verification.elapsed_ms);
    }
    Ok(DispatchResult {
        command: "auth.login".into(),
        data: json!({
            "profile": profile,
            "stored": true,
            "verified": verification.is_some(),
            "identity": identity,
        }),
        meta,
        failure_code: None,
    })
}

fn prompt_setup(
    config: &ConfigFile,
    selected_profile: Option<&str>,
    explicit_url: Option<&str>,
) -> Result<SetupDraft, AppError> {
    let profile = prompt_profile(config, selected_profile)?;
    let default_url = explicit_url
        .map(str::to_owned)
        .or_else(|| config.profiles.get(&profile).map(|value| value.url.clone()))
        .or_else(|| std::env::var("SIGNOZ_URL").ok())
        .unwrap_or_else(|| "http://localhost:3301".into());
    let url: String = prompt_io(
        cliclack::input("SigNoz instance URL")
            .default_input(&default_url)
            .validate(|value: &String| SigNozOrigin::parse(value).map(|_| ()))
            .interact(),
    )?;
    let api_key = SecretString::from(prompt_io(
        cliclack::password("SigNoz API key")
            .validate(|value: &String| {
                (!value.trim().is_empty())
                    .then_some(())
                    .ok_or("API key is required")
            })
            .interact(),
    )?);
    let skill_target = prompt_io(
        cliclack::select("Install the SigNoz Skill for coding agents?")
            .item(
                SkillChoice::All,
                "Codex and Claude Code",
                "Recommended; installs both Skills",
            )
            .item(SkillChoice::Codex, "Codex only", "Install for Codex")
            .item(
                SkillChoice::Claude,
                "Claude Code only",
                "Install for Claude Code",
            )
            .item(SkillChoice::Skip, "Skip", "Do not install Skills now")
            .initial_value(SkillChoice::All)
            .interact(),
    )?;

    prompt_io(cliclack::note(
        "Configuration summary",
        format!(
            "Profile: {profile}\nURL: {url}\nSkills: {}\nCredential: stored in the operating-system credential store",
            skill_target.label()
        ),
    ))?;
    if !prompt_io(
        cliclack::confirm("Verify and save this configuration?")
            .initial_value(true)
            .interact(),
    )? {
        return cancelled("Setup cancelled; no configuration was saved");
    }

    Ok(SetupDraft {
        profile,
        url,
        api_key,
        skill_target,
    })
}

fn prompt_profile(config: &ConfigFile, selected_profile: Option<&str>) -> Result<String, AppError> {
    if let Some(name) = selected_profile {
        if config.profiles.contains_key(name) {
            return Ok(name.to_owned());
        }
        validate_profile_name(name).map_err(AppError::validation)?;
        return Ok(name.to_owned());
    }

    let choice = if config.profiles.is_empty() {
        ProfileChoice::Create
    } else {
        let mut prompt = cliclack::select("Which profile should be configured?").item(
            ProfileChoice::Create,
            "Create a new profile",
            "Add another SigNoz instance",
        );
        for (name, profile) in &config.profiles {
            prompt = prompt.item(ProfileChoice::Existing(name.clone()), name, &profile.url);
        }
        if let Some(current) = config.current_profile.as_ref() {
            prompt = prompt.initial_value(ProfileChoice::Existing(current.clone()));
        }
        prompt_io(prompt.interact())?
    };

    match choice {
        ProfileChoice::Existing(name) => Ok(name),
        ProfileChoice::Create => prompt_io(
            cliclack::input("Profile name")
                .default_input("default")
                .validate(|value: &String| validate_profile_name(value))
                .interact(),
        ),
    }
}

fn select_login_profile(
    config: &ConfigFile,
    selected_profile: Option<&str>,
    interactive: bool,
) -> Result<String, AppError> {
    if let Some(name) = selected_profile {
        return Ok(name.to_owned());
    }
    if let Some(name) = &config.current_profile {
        return Ok(name.clone());
    }
    if config.profiles.len() == 1 {
        return Ok(config.profiles.keys().next().expect("one profile").clone());
    }
    if config.profiles.is_empty() {
        return Err(AppError::config("no profiles are configured")
            .with_hint("Run `signoz-cli init` or `signoz-cli profile add <name> --url <url>`"));
    }
    if !interactive {
        return Err(
            AppError::config("multiple profiles exist and none is selected")
                .with_hint("Pass `--profile <name>` or run `signoz-cli profile use <name>`"),
        );
    }

    let mut prompt = cliclack::select("Which profile should receive the API key?");
    for (name, profile) in &config.profiles {
        prompt = prompt.item(name.clone(), name, &profile.url);
    }
    prompt_io(prompt.interact())
}

async fn verify_key(
    url: &str,
    api_key: &str,
    timeout_seconds: u64,
) -> Result<Verification, AppError> {
    let origin = SigNozOrigin::parse(url).map_err(AppError::validation)?;
    let client = SigNozClient::new_with_timeout(
        origin,
        Credentials::new(api_key),
        Duration::from_secs(timeout_seconds.max(1)),
    )?;
    let started = Instant::now();
    let mut response = client.execute(Operation::auth_me()).await?;
    project_auth_identity(&mut response.value)?;
    Ok(Verification {
        identity: response.value,
        status: response.meta.status,
        request_id: response.meta.request_id,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

fn persist_setup(
    config: &mut ConfigFile,
    config_path: &Path,
    store: &dyn CredentialStore,
    draft: &SetupDraft,
) -> Result<(), AppError> {
    let previous_config = config.clone();
    let previous_key = store.get(&draft.profile).map_err(AppError::config)?;
    config
        .upsert_profile(&draft.profile, &draft.url)
        .map_err(AppError::validation)?;
    config
        .use_profile(&draft.profile)
        .map_err(AppError::config)?;
    store
        .set(&draft.profile, draft.api_key.expose_secret())
        .map_err(AppError::config)?;

    if let Err(save_error) = config.save(config_path) {
        *config = previous_config;
        let rollback = match previous_key {
            Some(previous) => store.set(&draft.profile, &previous),
            None => store.delete(&draft.profile),
        };
        return Err(AppError::config(match rollback {
            Ok(()) => format!(
                "failed to save configuration; credential change was rolled back: {save_error}"
            ),
            Err(rollback_error) => format!(
                "failed to save configuration ({save_error}) and failed to restore the previous credential ({rollback_error})"
            ),
        }));
    }
    Ok(())
}

fn interactive_terminal() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal() && io::stderr().is_terminal()
}

fn read_api_key_from_stdin() -> Result<String, AppError> {
    read_api_key(&mut io::stdin())
}

fn read_api_key(reader: &mut impl Read) -> Result<String, AppError> {
    const MAX_API_KEY_BYTES: u64 = 64 * 1024;
    let mut value = String::new();
    reader
        .take(MAX_API_KEY_BYTES + 1)
        .read_to_string(&mut value)
        .map_err(|error| AppError::config(error.to_string()))?;
    if value.len() as u64 > MAX_API_KEY_BYTES {
        return Err(AppError::validation(format!(
            "API key from stdin exceeds the {MAX_API_KEY_BYTES}-byte limit"
        )));
    }
    let value = value
        .strip_suffix("\r\n")
        .or_else(|| value.strip_suffix('\n'))
        .unwrap_or(&value)
        .to_owned();
    if value.is_empty() {
        return Err(AppError::validation("API key from stdin is empty"));
    }
    if value.contains(['\r', '\n', '\0']) {
        return Err(AppError::validation(
            "API key from stdin must be exactly one non-empty line",
        ));
    }
    Ok(value)
}

fn prompt_io<T>(result: io::Result<T>) -> Result<T, AppError> {
    result.map_err(|error| {
        if error.kind() == io::ErrorKind::Interrupted {
            AppError::validation("operation cancelled")
        } else {
            AppError::config(format!("terminal interaction failed: {error}"))
        }
    })
}

fn cancelled<T>(message: &str) -> Result<T, AppError> {
    let _ = cliclack::outro_cancel(message);
    Err(AppError::validation(message))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

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

    fn draft(key: &str) -> SetupDraft {
        SetupDraft {
            profile: "prod".into(),
            url: "https://signoz.example.com".into(),
            api_key: SecretString::from(key.to_owned()),
            skill_target: SkillChoice::Skip,
        }
    }

    #[test]
    fn setup_persists_profile_without_serializing_the_api_key() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let store = MemoryStore::default();
        let mut config = ConfigFile::default();

        persist_setup(&mut config, &path, &store, &draft("top-secret")).unwrap();

        assert_eq!(config.current_profile.as_deref(), Some("prod"));
        assert_eq!(store.get("prod").unwrap().as_deref(), Some("top-secret"));
        assert!(
            !std::fs::read_to_string(path)
                .unwrap()
                .contains("top-secret")
        );
    }

    #[test]
    fn failed_config_save_restores_the_previous_credential_and_config() {
        let directory = tempfile::tempdir().unwrap();
        let blocker = directory.path().join("not-a-directory");
        std::fs::write(&blocker, "block").unwrap();
        let path = blocker.join("config.toml");
        let store = MemoryStore::default();
        store.set("prod", "old-secret").unwrap();
        let mut config = ConfigFile::default();
        config
            .add_profile("prod", "https://old.signoz.example.com")
            .unwrap();

        let error = persist_setup(&mut config, &path, &store, &draft("new-secret")).unwrap_err();

        assert!(error.message.contains("rolled back"));
        assert_eq!(store.get("prod").unwrap().as_deref(), Some("old-secret"));
        assert_eq!(
            config.profiles["prod"].url,
            "https://old.signoz.example.com"
        );
        assert_eq!(config.current_profile, None);
    }

    #[test]
    fn piped_api_keys_accept_one_line_and_trim_only_its_ending() {
        let mut input = io::Cursor::new(b"  key-with-spaces  \r\n");

        let key = read_api_key(&mut input).unwrap();

        assert_eq!(key, "  key-with-spaces  ");
    }

    #[test]
    fn piped_api_keys_reject_empty_multiline_and_oversized_input() {
        for value in [b"".as_slice(), b"\n", b"first\nsecond\n"] {
            let error = read_api_key(&mut io::Cursor::new(value)).unwrap_err();
            assert_eq!(error.code, signoz_client::ErrorCode::Validation);
        }
        let oversized = vec![b'x'; 64 * 1024 + 1];
        let error = read_api_key(&mut io::Cursor::new(oversized)).unwrap_err();
        assert!(error.message.contains("exceeds"));
    }
}
