from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
SDK_WORKFLOW_PATH = REPO_ROOT / ".github/workflows/sdk.yml"


class SdkWorkflowTests(unittest.TestCase):
    def test_sdk_workflow_gives_forks_more_time_for_local_bazel_builds(self) -> None:
        workflow = SDK_WORKFLOW_PATH.read_text()

        self.assertIn(
            "timeout-minutes: ${{ github.repository == 'openai/codex' && 10 || 30 }}",
            workflow,
        )


if __name__ == "__main__":
    unittest.main()
