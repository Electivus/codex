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

    def test_remote_fork_retries_locally_when_buildbuddy_dns_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            bazel_calls_dir = temp_path / "bazel-calls"
            bazel_calls_dir.mkdir()
            bazel_call_count_file = temp_path / "bazel-call-count.txt"
            fake_bazel = temp_path / "bazel"
            fake_bazel.write_text(
                "\n".join(
                    [
                        "#!/usr/bin/env bash",
                        "set -euo pipefail",
                        'count=0',
                        'if [[ -f "$FAKE_BAZEL_CALL_COUNT_FILE" ]]; then',
                        '  count="$(cat "$FAKE_BAZEL_CALL_COUNT_FILE")"',
                        "fi",
                        'count=$((count + 1))',
                        'printf "%s\\n" "$count" > "$FAKE_BAZEL_CALL_COUNT_FILE"',
                        'printf "%s\\n" "$@" > "$FAKE_BAZEL_CALLS_DIR/call-${count}.txt"',
                        'has_remote_header=0',
                        'for arg in "$@"; do',
                        '  if [[ "$arg" == "--remote_header=x-buildbuddy-api-key=test-key" ]]; then',
                        '    has_remote_header=1',
                        '    break',
                        "  fi",
                        "done",
                        'if [[ $has_remote_header -eq 1 ]]; then',
                        '  echo "ERROR: fake target failed: Failed to query remote execution capabilities: UNAVAILABLE: Unable to resolve host remote.buildbuddy.io" >&2',
                        "  exit 1",
                        "fi",
                    ]
                )
                + "\n"
            )
            fake_bazel.chmod(fake_bazel.stat().st_mode | stat.S_IXUSR)

            env = os.environ.copy()
            env["BUILDBUDDY_API_KEY"] = "test-key"
            env["FAKE_BAZEL_CALLS_DIR"] = str(bazel_calls_dir)
            env["FAKE_BAZEL_CALL_COUNT_FILE"] = str(bazel_call_count_file)
            env["GITHUB_REPOSITORY"] = "Electivus/codex"
            env["PATH"] = f"{temp_path}:{env['PATH']}"
            env["RUNNER_OS"] = "macOS"

            result = subprocess.run(
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

            self.assertEqual(bazel_call_count_file.read_text().strip(), "2")
            first_call_args = (bazel_calls_dir / "call-1.txt").read_text().splitlines()
            second_call_args = (bazel_calls_dir / "call-2.txt").read_text().splitlines()
            self.assertIn("--remote_header=x-buildbuddy-api-key=test-key", first_call_args)
            self.assertNotIn("--remote_header=x-buildbuddy-api-key=test-key", second_call_args)
            self.assertIn("--remote_cache=", second_call_args)
            self.assertIn("--remote_executor=", second_call_args)
            self.assertIn("--experimental_remote_downloader=", second_call_args)
            self.assertIn(
                "BuildBuddy remote execution is unavailable; retrying once with local Bazel configuration.",
                result.stdout,
            )

    def test_remote_macos_fork_builds_reduce_jobs_for_stability(self) -> None:
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
            self.assertIn("--jobs=10", bazel_args)

    def test_remote_linux_fork_tests_reduce_jobs_and_local_test_jobs(self) -> None:
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
            env["RUNNER_OS"] = "Linux"

            subprocess.run(
                [
                    "bash",
                    str(SCRIPT_PATH),
                    "--",
                    "test",
                    "--build_metadata=COMMIT_SHA=test-sha",
                    "--",
                    "//codex-rs/core:core-all-test",
                ],
                cwd=REPO_ROOT,
                env=env,
                check=True,
                text=True,
                capture_output=True,
            )

            bazel_args = bazel_args_file.read_text().splitlines()
            self.assertIn("--jobs=10", bazel_args)
            self.assertIn("--local_test_jobs=2", bazel_args)

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
            self.assertNotIn("--jobs=10", bazel_args)

    def test_remote_fork_tests_respect_local_test_jobs_override(self) -> None:
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
            env["RUNNER_OS"] = "Windows"

            subprocess.run(
                [
                    "bash",
                    str(SCRIPT_PATH),
                    "--",
                    "test",
                    "--local_test_jobs=7",
                    "--build_metadata=COMMIT_SHA=test-sha",
                    "--",
                    "//codex-rs/app-server:app-server-all-test",
                ],
                cwd=REPO_ROOT,
                env=env,
                check=True,
                text=True,
                capture_output=True,
            )

            bazel_args = bazel_args_file.read_text().splitlines()
            self.assertIn("--local_test_jobs=7", bazel_args)
            self.assertNotIn("--local_test_jobs=2", bazel_args)


if __name__ == "__main__":
    unittest.main()
