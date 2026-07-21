from __future__ import annotations

import importlib.util
import json
import shutil
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "validate_agent_integration", ROOT / "scripts/validate-agent-integration.py"
)
VALIDATOR = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(VALIDATOR)


class AgentIntegrationValidationTest(unittest.TestCase):
    def fixture(self) -> tuple[tempfile.TemporaryDirectory, Path]:
        temporary = tempfile.TemporaryDirectory()
        root = Path(temporary.name)
        for relative in ["plugins", ".agents", ".claude-plugin", "tests/skill"]:
            source = ROOT / relative
            destination = root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copytree(source, destination)
        return temporary, root

    def test_live_tree_is_valid(self) -> None:
        self.assertEqual(VALIDATOR.validate(ROOT), [])

    def test_rejects_broken_skill_and_manifest_contracts(self) -> None:
        mutations = [
            ("missing name", lambda root: self.replace(root, "name: signoz-cli", "name:")),
            ("name mismatch", lambda root: self.replace(root, "name: signoz-cli", "name: other")),
            ("long description", self.long_description),
            ("missing command", lambda root: self.replace(root, "signoz-cli ", "observe ")),
            (
                "missing doctor workflow",
                lambda root: self.replace(root, "signoz-cli doctor", "signoz-cli inspect"),
            ),
            (
                "forbidden key flag",
                lambda root: self.append_skill(root, "\nsignoz-cli --api-key secret\n"),
            ),
            (
                "legacy command",
                lambda root: self.append_skill(root, "\nsignoz version --output json\n"),
            ),
            (
                "pipeline legacy command",
                lambda root: self.append_skill(
                    root, "\nprintf '%s' x | signoz version --output json\n"
                ),
            ),
            (
                "array legacy command",
                lambda root: self.append_skill(
                    root, "\nargs=(signoz version --output json)\n"
                ),
            ),
            (
                "prompt-prefix legacy command",
                lambda root: self.append_skill(
                    root, "\n$ signoz version --output json\n"
                ),
            ),
            (
                "multiline forbidden key flag",
                lambda root: self.append_skill(
                    root, "\nsignoz-cli logs search \\\n  --api-key secret\n"
                ),
            ),
            ("version mismatch", self.version_mismatch),
            ("missing target", self.missing_target),
            ("trigger overlap", self.trigger_overlap),
            ("implicit invocation disabled", self.implicit_invocation_disabled),
            ("unimplemented scenario expectation", self.unimplemented_scenario),
        ]
        for label, mutate in mutations:
            with self.subTest(label=label):
                temporary, root = self.fixture()
                try:
                    mutate(root)
                    self.assertTrue(VALIDATOR.validate(root), label)
                finally:
                    temporary.cleanup()

    @staticmethod
    def skill(root: Path) -> Path:
        return root / "plugins/signoz-cli/skills/signoz-cli/SKILL.md"

    def replace(self, root: Path, old: str, new: str) -> None:
        path = self.skill(root)
        path.write_text(path.read_text().replace(old, new), encoding="utf-8")

    def append_skill(self, root: Path, value: str) -> None:
        path = self.skill(root)
        path.write_text(path.read_text() + value, encoding="utf-8")

    def long_description(self, root: Path) -> None:
        self.replace(root, "description: ", "description: " + "x" * 1100)

    @staticmethod
    def version_mismatch(root: Path) -> None:
        path = root / "plugins/signoz-cli/.claude-plugin/plugin.json"
        value = json.loads(path.read_text())
        value["version"] = "9.9.9"
        path.write_text(json.dumps(value), encoding="utf-8")

    @staticmethod
    def missing_target(root: Path) -> None:
        path = root / ".claude-plugin/marketplace.json"
        value = json.loads(path.read_text())
        value["plugins"][0]["source"] = "./missing"
        path.write_text(json.dumps(value), encoding="utf-8")

    def trigger_overlap(self, root: Path) -> None:
        self.replace(root, "Do not trigger", "Always trigger")

    @staticmethod
    def implicit_invocation_disabled(root: Path) -> None:
        path = root / "plugins/signoz-cli/skills/signoz-cli/agents/openai.yaml"
        path.write_text(
            path.read_text().replace(
                "allow_implicit_invocation: true", "allow_implicit_invocation: false"
            ),
            encoding="utf-8",
        )

    @staticmethod
    def unimplemented_scenario(root: Path) -> None:
        path = root / "tests/skill/scenarios.json"
        value = json.loads(path.read_text())
        value[0]["mustInclude"].append("invented-command")
        path.write_text(json.dumps(value), encoding="utf-8")


if __name__ == "__main__":
    unittest.main()
