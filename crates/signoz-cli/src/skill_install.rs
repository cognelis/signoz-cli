use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

use directories::BaseDirs;
use serde::Serialize;
use tempfile::NamedTempFile;

use crate::args::SkillTarget;

const SKILL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../plugins/signoz-cli/skills/signoz-cli/SKILL.md"
));
const OPENAI_METADATA: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../plugins/signoz-cli/skills/signoz-cli/agents/openai.yaml"
));

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Codex,
    Claude,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallStatus {
    Installed,
    Updated,
    Unchanged,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub agent: Agent,
    pub path: String,
    pub status: InstallStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InstallTarget {
    agent: Agent,
    directory: PathBuf,
}

struct ManagedFile {
    relative_path: &'static str,
    contents: &'static [u8],
}

const CODEX_FILES: &[ManagedFile] = &[
    ManagedFile {
        relative_path: "SKILL.md",
        contents: SKILL,
    },
    ManagedFile {
        relative_path: "agents/openai.yaml",
        contents: OPENAI_METADATA,
    },
];
const CLAUDE_FILES: &[ManagedFile] = &[ManagedFile {
    relative_path: "SKILL.md",
    contents: SKILL,
}];

pub fn install(target: SkillTarget) -> Result<Vec<InstallResult>, String> {
    let home = env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| BaseDirs::new().map(|directories| directories.home_dir().to_path_buf()));
    let claude_config = env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let targets = resolve_targets(target, home.as_deref(), claude_config.as_deref())?;

    install_targets(&targets)
}

fn resolve_targets(
    target: SkillTarget,
    home: Option<&Path>,
    claude_config: Option<&Path>,
) -> Result<Vec<InstallTarget>, String> {
    let home = home.filter(|path| !path.as_os_str().is_empty());
    let claude_config = claude_config.filter(|path| !path.as_os_str().is_empty());

    match target {
        SkillTarget::All => {
            let home = required_home(home, Agent::Codex)?;
            Ok(vec![codex_target(home), claude_target(home, claude_config)])
        }
        SkillTarget::Codex => Ok(vec![codex_target(required_home(home, Agent::Codex)?)]),
        SkillTarget::Claude => {
            let directory = match claude_config {
                Some(config) => config.join("skills/signoz-cli"),
                None => required_home(home, Agent::Claude)?.join(".claude/skills/signoz-cli"),
            };
            Ok(vec![InstallTarget {
                agent: Agent::Claude,
                directory,
            }])
        }
    }
}

fn required_home(home: Option<&Path>, agent: Agent) -> Result<&Path, String> {
    home.ok_or_else(|| {
        format!(
            "could not determine the home directory required for {} Skill installation",
            agent.name()
        )
    })
}

fn codex_target(home: &Path) -> InstallTarget {
    InstallTarget {
        agent: Agent::Codex,
        directory: home.join(".agents/skills/signoz-cli"),
    }
}

fn claude_target(home: &Path, claude_config: Option<&Path>) -> InstallTarget {
    InstallTarget {
        agent: Agent::Claude,
        directory: claude_config
            .map(|config| config.join("skills/signoz-cli"))
            .unwrap_or_else(|| home.join(".claude/skills/signoz-cli")),
    }
}

fn install_targets(targets: &[InstallTarget]) -> Result<Vec<InstallResult>, String> {
    targets.iter().map(install_target).collect()
}

fn install_target(target: &InstallTarget) -> Result<InstallResult, String> {
    let managed_files = target.agent.managed_files();
    let skill_path = target.directory.join("SKILL.md");
    let skill_was_present = path_exists(&skill_path)?;
    let mut changes = Vec::new();

    for managed_file in managed_files {
        let path = target.directory.join(managed_file.relative_path);
        match fs::read(&path) {
            Ok(existing) if existing == managed_file.contents => {}
            Ok(_) => changes.push((path, managed_file.contents)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                changes.push((path, managed_file.contents));
            }
            Err(error) => return Err(path_error("read", &path, &error)),
        }
    }

    if changes.is_empty() {
        return Ok(InstallResult {
            agent: target.agent,
            path: target.directory.to_string_lossy().into_owned(),
            status: InstallStatus::Unchanged,
        });
    }

    for (path, contents) in changes {
        let parent = path.parent().ok_or_else(|| {
            format!(
                "managed Skill file has no parent directory: {}",
                path.display()
            )
        })?;
        fs::create_dir_all(parent)
            .map_err(|error| path_error("create directory for", &path, &error))?;
        atomic_write(&path, contents)?;
    }

    Ok(InstallResult {
        agent: target.agent,
        path: target.directory.to_string_lossy().into_owned(),
        status: if skill_was_present {
            InstallStatus::Updated
        } else {
            InstallStatus::Installed
        },
    })
}

fn path_exists(path: &Path) -> Result<bool, String> {
    match fs::metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(path_error("inspect", path, &error)),
    }
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| {
        format!(
            "managed Skill file has no parent directory: {}",
            path.display()
        )
    })?;
    let mut temporary = NamedTempFile::new_in(parent)
        .map_err(|error| path_error("create temporary file for", path, &error))?;
    temporary
        .write_all(contents)
        .map_err(|error| path_error("write", path, &error))?;
    temporary
        .flush()
        .map_err(|error| path_error("flush", path, &error))?;
    temporary
        .persist(path)
        .map_err(|error| path_error("persist", path, &error.error))?;
    Ok(())
}

fn path_error(action: &str, path: &Path, error: &std::io::Error) -> String {
    format!("failed to {action} {}: {error}", path.display())
}

impl Agent {
    fn name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
        }
    }

    fn managed_files(self) -> &'static [ManagedFile] {
        match self {
            Self::Codex => CODEX_FILES,
            Self::Claude => CLAUDE_FILES,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use crate::args::SkillTarget;

    use super::{Agent, InstallStatus, install_targets, resolve_targets};

    #[test]
    fn all_resolves_codex_then_claude_under_home() {
        let home = Path::new("/home/tester");

        let targets = resolve_targets(SkillTarget::All, Some(home), None).unwrap();

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].agent, Agent::Codex);
        assert_eq!(targets[0].directory, home.join(".agents/skills/signoz-cli"));
        assert_eq!(targets[1].agent, Agent::Claude);
        assert_eq!(targets[1].directory, home.join(".claude/skills/signoz-cli"));
    }

    #[test]
    fn claude_config_directory_works_without_home() {
        let config = Path::new("/custom/claude");

        let targets = resolve_targets(SkillTarget::Claude, None, Some(config)).unwrap();

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].agent, Agent::Claude);
        assert_eq!(targets[0].directory, config.join("skills/signoz-cli"));
    }

    #[test]
    fn missing_required_home_errors_before_writing() {
        let claude_config = tempfile::tempdir().unwrap();

        let error = resolve_targets(SkillTarget::All, None, Some(claude_config.path()))
            .expect_err("Codex requires a home directory");

        assert!(
            error.contains("home directory"),
            "unexpected error: {error}"
        );
        assert!(!claude_config.path().join("skills").exists());
    }

    #[test]
    fn install_reports_installed_updated_then_unchanged() {
        let home = tempfile::tempdir().unwrap();
        let targets = resolve_targets(SkillTarget::Codex, Some(home.path()), None).unwrap();

        assert_eq!(
            install_targets(&targets).unwrap()[0].status,
            InstallStatus::Installed
        );
        fs::write(targets[0].directory.join("SKILL.md"), "stale").unwrap();
        assert_eq!(
            install_targets(&targets).unwrap()[0].status,
            InstallStatus::Updated
        );
        assert_eq!(
            install_targets(&targets).unwrap()[0].status,
            InstallStatus::Unchanged
        );
    }

    #[test]
    fn install_preserves_unmanaged_files() {
        let home = tempfile::tempdir().unwrap();
        let targets = resolve_targets(SkillTarget::Codex, Some(home.path()), None).unwrap();
        fs::create_dir_all(&targets[0].directory).unwrap();
        let notes = targets[0].directory.join("notes.txt");
        fs::write(&notes, "keep me").unwrap();

        install_targets(&targets).unwrap();

        assert_eq!(fs::read_to_string(notes).unwrap(), "keep me");
    }

    #[test]
    fn installs_skill_for_both_and_openai_metadata_only_for_codex() {
        let home = tempfile::tempdir().unwrap();
        let targets = resolve_targets(SkillTarget::All, Some(home.path()), None).unwrap();

        install_targets(&targets).unwrap();

        assert_eq!(
            fs::read(targets[0].directory.join("SKILL.md")).unwrap(),
            super::SKILL
        );
        assert_eq!(
            fs::read(targets[0].directory.join("agents/openai.yaml")).unwrap(),
            super::OPENAI_METADATA
        );
        assert_eq!(
            fs::read(targets[1].directory.join("SKILL.md")).unwrap(),
            super::SKILL
        );
        assert!(!targets[1].directory.join("agents/openai.yaml").exists());
    }

    #[test]
    fn results_serialize_as_expected_objects() {
        let home = tempfile::tempdir().unwrap();
        let targets = resolve_targets(SkillTarget::All, Some(home.path()), None).unwrap();

        let value = serde_json::to_value(install_targets(&targets).unwrap()).unwrap();

        assert_eq!(value[0]["agent"], "codex");
        assert_eq!(value[0]["path"].as_str(), targets[0].directory.to_str());
        assert_eq!(value[0]["status"], "installed");
        assert_eq!(value[1]["agent"], "claude");
        assert_eq!(value[1]["path"].as_str(), targets[1].directory.to_str());
        assert_eq!(value[1]["status"], "installed");
        assert_eq!(value[0].as_object().unwrap().len(), 3);
        assert_eq!(value[1].as_object().unwrap().len(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn result_path_serializes_when_target_path_is_not_utf8() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let root = tempfile::tempdir().unwrap();
        let home = root
            .path()
            .join(OsString::from_vec(vec![b'h', b'o', b'm', b'e', 0xff]));
        let targets = resolve_targets(SkillTarget::Codex, Some(&home), None).unwrap();

        #[cfg(not(target_os = "macos"))]
        let results = install_targets(&targets).unwrap();

        // macOS rejects invalid UTF-8 in filesystem operations, so install into
        // the valid temporary root before exercising the resolved report path.
        #[cfg(target_os = "macos")]
        let results = {
            use std::path::PathBuf;

            trait ReportPathForTest {
                fn from_directory(directory: &Path) -> Self;
            }

            impl ReportPathForTest for PathBuf {
                fn from_directory(directory: &Path) -> Self {
                    directory.to_path_buf()
                }
            }

            impl ReportPathForTest for String {
                fn from_directory(directory: &Path) -> Self {
                    directory.to_string_lossy().into_owned()
                }
            }

            let install_targets_resolved =
                resolve_targets(SkillTarget::Codex, Some(root.path()), None).unwrap();
            let mut results = install_targets(&install_targets_resolved).unwrap();
            results[0].path = ReportPathForTest::from_directory(&targets[0].directory);
            results
        };

        let value = serde_json::to_value(&results)
            .expect("reported paths use a guaranteed UTF-8 representation");

        assert!(value[0]["path"].is_string());
        assert_eq!(
            value[0]["path"].as_str(),
            Some(targets[0].directory.to_string_lossy().as_ref())
        );
    }
}
