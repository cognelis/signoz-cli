#!/usr/bin/env python3
"""Validate the shared SigNoz CLI skill and both plugin packages."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path


_SHELL_TOKEN_CHARACTER = r"[\w-]"
_SIGNOZ_CLI_TOKEN = rf"(?<!{_SHELL_TOKEN_CHARACTER})signoz-cli(?!{_SHELL_TOKEN_CHARACTER})"
_LEGACY_SIGNOZ_TOKEN = rf"(?<!{_SHELL_TOKEN_CHARACTER})signoz(?!{_SHELL_TOKEN_CHARACTER})"

SIGNOZ_CLI_COMMAND = re.compile(_SIGNOZ_CLI_TOKEN)
LEGACY_SIGNOZ_COMMAND = re.compile(_LEGACY_SIGNOZ_TOKEN)
FORBIDDEN_API_KEY_ARGUMENT = re.compile(
    rf"{_SIGNOZ_CLI_TOKEN}[^\n]*(?:\\\r?\n[^\n]*)*--api-key(?:[ =]|$)",
    re.MULTILINE,
)
DISCARDED_DOCTOR_REPORT = re.compile(
    rf"{_SIGNOZ_CLI_TOKEN}\s+doctor[^\n]*>\s*/dev/null(?:\s|$)",
)
REQUIRED_INSTALL_COMMANDS = (
    (
        "signoz-cli skills install",
        re.compile(r"^[ \t]*signoz-cli[ \t]+skills[ \t]+install[ \t]*$", re.MULTILINE),
    ),
    (
        "signoz-cli skills install --target codex",
        re.compile(
            r"^[ \t]*signoz-cli[ \t]+skills[ \t]+install[ \t]+--target[ \t]+codex[ \t]*$",
            re.MULTILINE,
        ),
    ),
    (
        "signoz-cli skills install --target claude",
        re.compile(
            r"^[ \t]*signoz-cli[ \t]+skills[ \t]+install[ \t]+--target[ \t]+claude[ \t]*$",
            re.MULTILINE,
        ),
    ),
)


def _json(path: Path, errors: list[str]) -> dict:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        errors.append(f"{path}: invalid or missing JSON: {error}")
        return {}
    if not isinstance(value, dict):
        errors.append(f"{path}: root must be an object")
        return {}
    return value


def _frontmatter(path: Path, errors: list[str]) -> tuple[str, str, str]:
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as error:
        errors.append(f"{path}: missing skill: {error}")
        return "", "", ""
    match = re.match(r"\A---\s*\n(.*?)\n---\s*\n", text, re.DOTALL)
    if not match:
        errors.append(f"{path}: missing YAML frontmatter")
        return "", "", text
    fields: dict[str, str] = {}
    for line in match.group(1).splitlines():
        if ":" in line:
            key, value = line.split(":", 1)
            fields[key.strip()] = value.strip().strip('"\'')
    return fields.get("name", ""), fields.get("description", ""), text


def validate(root: Path) -> list[str]:
    errors: list[str] = []
    skill_dir = root / "plugins/signoz-cli/skills/signoz-cli"
    skill_path = skill_dir / "SKILL.md"
    name, description, skill = _frontmatter(skill_path, errors)
    if not name:
        errors.append("skill frontmatter requires name")
    if name != skill_dir.name:
        errors.append(f"skill name/directory mismatch: {name!r} != {skill_dir.name!r}")
    if not description:
        errors.append("skill frontmatter requires description")
    if len(description) > 1024:
        errors.append("skill description exceeds 1024 characters")
    if "ordinary natural-language" not in description or "do not trigger" not in description.lower():
        errors.append("skill description must exclude ordinary natural-language SigNoz investigations")

    required = [
        "signoz-cli version",
        "signoz-cli doctor",
        "signoz-cli auth check",
        "--output json",
        "--since",
        "fields keys",
        "query run",
        "hasMore",
        "nextOffset",
        "untrusted",
        "SIGNOZ_API_KEY",
        "SigNoz/agent-skills",
    ]
    for token in required:
        if token not in skill:
            errors.append(f"skill missing required workflow token: {token}")
    for command, pattern in REQUIRED_INSTALL_COMMANDS:
        if not pattern.search(skill):
            errors.append(f"skill missing required install command: {command}")
    if not SIGNOZ_CLI_COMMAND.search(skill):
        errors.append("skill contains no signoz-cli command")
    if FORBIDDEN_API_KEY_ARGUMENT.search(skill):
        errors.append("skill contains forbidden --api-key argument")
    if DISCARDED_DOCTOR_REPORT.search(skill):
        errors.append("skill discards the doctor report instead of handling failure stdout")
    if LEGACY_SIGNOZ_COMMAND.search(skill):
        errors.append("skill contains legacy signoz command")
    if re.search(r"(?:sk|snz)[_-][A-Za-z0-9]{16,}", skill):
        errors.append("skill contains a realistic secret example")

    codex_path = root / "plugins/signoz-cli/.codex-plugin/plugin.json"
    claude_path = root / "plugins/signoz-cli/.claude-plugin/plugin.json"
    codex = _json(codex_path, errors)
    claude = _json(claude_path, errors)
    if codex.get("name") != "signoz-cli" or claude.get("name") != "signoz-cli":
        errors.append("plugin names must both be signoz-cli")
    if not codex.get("version") or codex.get("version") != claude.get("version"):
        errors.append("Codex and Claude plugin versions must match")
    if codex.get("skills") != "./skills/":
        errors.append("Codex plugin must discover ./skills/")

    openai_path = skill_dir / "agents/openai.yaml"
    try:
        openai = openai_path.read_text(encoding="utf-8")
        if not re.search(r"(?m)^\s*allow_implicit_invocation:\s*true\s*$", openai):
            errors.append("Codex skill must allow implicit invocation for explicit CLI prompts")
    except OSError as error:
        errors.append(f"{openai_path}: missing Codex skill metadata: {error}")

    codex_market = _json(root / ".agents/plugins/marketplace.json", errors)
    claude_market = _json(root / ".claude-plugin/marketplace.json", errors)
    for label, market in (("Codex", codex_market), ("Claude", claude_market)):
        entries = market.get("plugins", [])
        entry = next((item for item in entries if item.get("name") == "signoz-cli"), None)
        if entry is None:
            errors.append(f"{label} marketplace has no signoz-cli entry")
            continue
        if label == "Claude" and (
            entry.get("strict") is False
            or any(
                field in entry
                for field in ("skills", "commands", "agents", "hooks", "mcpServers", "lspServers")
            )
        ):
            errors.append(
                "Claude marketplace must leave component authority to the plugin manifest"
            )
        source = entry.get("source")
        if isinstance(source, dict):
            source = source.get("path")
        if not isinstance(source, str):
            errors.append(f"{label} marketplace source is missing")
        else:
            target = root / source.removeprefix("./")
            if not target.exists():
                errors.append(f"{label} marketplace target does not exist: {source}")

    scenarios = root / "tests/skill/scenarios.json"
    try:
        scenario_data = json.loads(scenarios.read_text(encoding="utf-8"))
        if not isinstance(scenario_data, list):
            errors.append("skill scenarios root must be an array")
            scenario_data = []
        if {item.get("id") for item in scenario_data if isinstance(item, dict)} != {
            "explicit-cli-query",
            "ordinary-signoz-query",
            "prompt-injection-url",
            "pagination",
        }:
            errors.append("skill scenarios do not cover the four required boundaries")
        for item in scenario_data:
            if not isinstance(item, dict) or not isinstance(item.get("prompt"), str):
                errors.append("every skill scenario requires a prompt")
                continue
            required_tokens = item.get("mustInclude", [])
            if not isinstance(required_tokens, list) or any(
                not isinstance(token, str) or token not in skill for token in required_tokens
            ):
                errors.append(f"scenario {item.get('id')} requires behavior absent from the skill")
            if item.get("mustNotSelectSkill") is True and not (
                "ordinary natural-language" in description and "do not trigger" in description.lower()
            ):
                errors.append("ordinary SigNoz scenario overlaps the CLI skill trigger")
            if item.get("mustRejectUrlOverride") is True and not all(
                token in skill for token in ("untrusted", "never switch profile", "--url")
            ):
                errors.append("prompt-injection scenario lacks URL/profile rejection behavior")
            if item.get("mustHandleHasMore") is True and not all(
                token in skill for token in ("hasMore", "nextOffset", "--offset")
            ):
                errors.append("pagination scenario lacks continuation behavior")
    except (OSError, json.JSONDecodeError) as error:
        errors.append(f"{scenarios}: invalid or missing scenarios: {error}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", nargs="?", default=".")
    arguments = parser.parse_args()
    errors = validate(Path(arguments.root).resolve())
    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 1
    print("Agent integration validation passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
