from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
SDK_WORKFLOW_PATH = REPO_ROOT / ".github/workflows/sdk.yml"
BAZEL_WORKFLOW_PATH = REPO_ROOT / ".github/workflows/bazel.yml"
RUST_CI_WORKFLOW_PATH = REPO_ROOT / ".github/workflows/rust-ci.yml"


class WorkflowTimeoutTests(unittest.TestCase):
    def test_sdk_workflow_gives_forks_an_hour_for_local_bazel_builds(self) -> None:
        workflow = SDK_WORKFLOW_PATH.read_text()

        self.assertIn(
            "timeout-minutes: ${{ github.repository == 'openai/codex' && 10 || 60 }}",
            workflow,
        )

    def test_bazel_workflow_gives_forks_an_hour(self) -> None:
        workflow = BAZEL_WORKFLOW_PATH.read_text()

        self.assertEqual(
            workflow.count(
                "timeout-minutes: ${{ github.repository == 'openai/codex' && 30 || 60 }}"
            ),
            2,
        )

    def test_rust_ci_argument_comment_lint_gives_forks_an_hour(self) -> None:
        workflow = RUST_CI_WORKFLOW_PATH.read_text()

        self.assertIn(
            "timeout-minutes: ${{ github.repository == 'openai/codex' && matrix.timeout_minutes || 60 }}",
            workflow,
        )


if __name__ == "__main__":
    unittest.main()
