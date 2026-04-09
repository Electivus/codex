import os
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / ".github/scripts/run-bazel-ci.sh"


class RunBazelCiTests(unittest.TestCase):
    def test_local_fallback_clears_remote_downloader_when_buildbuddy_key_missing(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            bazel_args_file = temp_path / "bazel-args.txt"
            fake_bazel = temp_path / "bazel"
            fake_bazel.write_text(
                "\n".join(
                    [
                        "#!/usr/bin/env bash",
                        "set -euo pipefail",
                        'printf "%s\\n" "$@" > "$FAKE_BAZEL_ARGS_FILE"',
                    ]
                )
                + "\n"
            )
            fake_bazel.chmod(fake_bazel.stat().st_mode | stat.S_IXUSR)

            env = os.environ.copy()
            env["FAKE_BAZEL_ARGS_FILE"] = str(bazel_args_file)
            env["PATH"] = f"{temp_path}:{env['PATH']}"
            env.pop("BUILDBUDDY_API_KEY", None)
            env["RUNNER_OS"] = "Linux"

            subprocess.run(
                [
                    "bash",
                    str(SCRIPT_PATH),
                    "--",
                    "build",
                    "--build_metadata=COMMIT_SHA=test-sha",
                    "--",
                    "//codex-rs/cli:codex",
                ],
                cwd=REPO_ROOT,
                env=env,
                check=True,
                text=True,
                capture_output=True,
            )

            bazel_args = bazel_args_file.read_text().splitlines()
            self.assertIn("--noexperimental_remote_repo_contents_cache", bazel_args)
            self.assertIn("--remote_cache=", bazel_args)
            self.assertIn("--remote_executor=", bazel_args)
            self.assertIn("--experimental_remote_downloader=", bazel_args)

    def test_remote_macos_fork_builds_override_remote_jobs(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            bazel_args_file = temp_path / "bazel-args.txt"
            fake_bazel = temp_path / "bazel"
            fake_bazel.write_text(
                "\n".join(
                    [
                        "#!/usr/bin/env bash",
                        "set -euo pipefail",
                        'printf "%s\\n" "$@" > "$FAKE_BAZEL_ARGS_FILE"',
                    ]
                )
                + "\n"
            )
            fake_bazel.chmod(fake_bazel.stat().st_mode | stat.S_IXUSR)

            env = os.environ.copy()
            env["BUILDBUDDY_API_KEY"] = "test-key"
            env["FAKE_BAZEL_ARGS_FILE"] = str(bazel_args_file)
            env["GITHUB_REPOSITORY"] = "Electivus/codex"
            env["PATH"] = f"{temp_path}:{env['PATH']}"
            env["RUNNER_OS"] = "macOS"

            subprocess.run(
                [
                    "bash",
                    str(SCRIPT_PATH),
                    "--",
                    "build",
                    "--build_metadata=COMMIT_SHA=test-sha",
                    "--",
                    "//codex-rs/cli:codex",
                ],
                cwd=REPO_ROOT,
                env=env,
                check=True,
                text=True,
                capture_output=True,
            )

            bazel_args = bazel_args_file.read_text().splitlines()
            self.assertIn("--config=ci-macos", bazel_args)
            self.assertIn("--remote_header=x-buildbuddy-api-key=test-key", bazel_args)
            self.assertIn("--jobs=30", bazel_args)

    def test_remote_macos_fork_builds_respect_short_jobs_override(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            bazel_args_file = temp_path / "bazel-args.txt"
            fake_bazel = temp_path / "bazel"
            fake_bazel.write_text(
                "\n".join(
                    [
                        "#!/usr/bin/env bash",
                        "set -euo pipefail",
                        'printf "%s\\n" "$@" > "$FAKE_BAZEL_ARGS_FILE"',
                    ]
                )
                + "\n"
            )
            fake_bazel.chmod(fake_bazel.stat().st_mode | stat.S_IXUSR)

            env = os.environ.copy()
            env["BUILDBUDDY_API_KEY"] = "test-key"
            env["FAKE_BAZEL_ARGS_FILE"] = str(bazel_args_file)
            env["GITHUB_REPOSITORY"] = "Electivus/codex"
            env["PATH"] = f"{temp_path}:{env['PATH']}"
            env["RUNNER_OS"] = "macOS"

            subprocess.run(
                [
                    "bash",
                    str(SCRIPT_PATH),
                    "--",
                    "build",
                    "-j=7",
                    "--build_metadata=COMMIT_SHA=test-sha",
                    "--",
                    "//codex-rs/cli:codex",
                ],
                cwd=REPO_ROOT,
                env=env,
                check=True,
                text=True,
                capture_output=True,
            )

            bazel_args = bazel_args_file.read_text().splitlines()
            self.assertIn("-j=7", bazel_args)
            self.assertNotIn("--jobs=30", bazel_args)


if __name__ == "__main__":
    unittest.main()
