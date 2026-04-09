from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
MODULE_BAZEL_PATH = REPO_ROOT / "MODULE.bazel"
PATCHES_BUILD_PATH = REPO_ROOT / "patches/BUILD.bazel"
AWS_LC_PATCH_PATH = (
    REPO_ROOT / "patches/aws-lc-sys_windows_msvc_compiler_checks.patch"
)


class AwsLcPatchTests(unittest.TestCase):
    def test_module_bazel_applies_windows_msvc_compiler_checks_patch(self) -> None:
        module = MODULE_BAZEL_PATH.read_text()

        self.assertIn(
            "//patches:aws-lc-sys_windows_msvc_compiler_checks.patch",
            module,
        )

    def test_patches_build_exports_windows_msvc_compiler_checks_patch(self) -> None:
        patches_build = PATCHES_BUILD_PATH.read_text()

        self.assertIn(
            '"aws-lc-sys_windows_msvc_compiler_checks.patch",',
            patches_build,
        )

    def test_windows_msvc_compiler_checks_patch_suppresses_c4100(self) -> None:
        patch = AWS_LC_PATCH_PATH.read_text()

        self.assertIn('cc_build.flag("/wd4100");', patch)
        self.assertIn("warnings_into_errors(true)", patch)


if __name__ == "__main__":
    unittest.main()
