import importlib.util
from pathlib import Path
import unittest
from unittest import mock


SCRIPT_PATH = Path(__file__).resolve().with_name("stage_npm_packages.py")
SPEC = importlib.util.spec_from_file_location("stage_npm_packages", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Unable to load module from {SCRIPT_PATH}")
stage_npm_packages = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stage_npm_packages)


class ResolveReleaseWorkflowTests(unittest.TestCase):
    def test_resolve_release_workflow_queries_canonical_repo(self) -> None:
        with mock.patch.object(
            stage_npm_packages.subprocess,
            "check_output",
            return_value='{"workflowName":"rust-release","url":"https://example.invalid/run","headSha":"abc123"}',
        ) as check_output:
            workflow = stage_npm_packages.resolve_release_workflow("0.115.0")

        self.assertEqual(
            workflow,
            {
                "workflowName": "rust-release",
                "url": "https://example.invalid/run",
                "headSha": "abc123",
            },
        )
        check_output.assert_called_once_with(
            [
                "gh",
                "run",
                "list",
                "--repo",
                stage_npm_packages.GITHUB_REPO,
                "--branch",
                "rust-v0.115.0",
                "--json",
                "workflowName,url,headSha",
                "--workflow",
                stage_npm_packages.WORKFLOW_NAME,
                "--jq",
                "first(.[])",
            ],
            cwd=stage_npm_packages.REPO_ROOT,
            text=True,
        )

    def test_resolve_release_workflow_raises_when_no_matching_run_exists(self) -> None:
        with mock.patch.object(stage_npm_packages.subprocess, "check_output", return_value=""):
            with self.assertRaisesRegex(
                RuntimeError,
                "Unable to find rust-release workflow for version 0.115.0.",
            ):
                stage_npm_packages.resolve_release_workflow("0.115.0")


if __name__ == "__main__":
    unittest.main()
