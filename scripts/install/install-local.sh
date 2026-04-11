#!/bin/sh

set -eu

INSTALL_DIR="${CODEX_INSTALL_DIR:-$HOME/.local/bin}"
BUILD_PROFILE="release"
path_action="already"
path_profile=""

usage() {
  cat <<'EOF'
Usage: install-local.sh [--debug|--release]

Build Codex from the current checkout and install the resulting binary locally.

Options:
  --debug    Build and install target/debug/codex
  --release  Build and install target/release/codex (default)
  -h, --help Show this help message

Environment:
  CODEX_INSTALL_DIR  Override the installation directory (default: ~/.local/bin)
EOF
}

step() {
  printf '==> %s\n' "$1"
}

warn() {
  printf 'Warning: %s\n' "$1" >&2
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf '%s is required to build and install Codex locally.\n' "$1" >&2
    exit 1
  fi
}

add_to_path() {
  path_action="already"
  path_profile=""

  case ":$PATH:" in
    *":$INSTALL_DIR:"*)
      return
      ;;
  esac

  profile="$HOME/.profile"
  case "${SHELL:-}" in
    */zsh)
      profile="$HOME/.zshrc"
      ;;
    */bash)
      profile="$HOME/.bashrc"
      ;;
  esac

  path_profile="$profile"
  path_line="export PATH=\"$INSTALL_DIR:\$PATH\""
  if [ -f "$profile" ] && grep -F "$path_line" "$profile" >/dev/null 2>&1; then
    path_action="configured"
    return
  fi

  {
    printf '\n# Added by Codex local installer\n'
    printf '%s\n' "$path_line"
  } >>"$profile"
  path_action="added"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --debug)
      BUILD_PROFILE="debug"
      ;;
    --release)
      BUILD_PROFILE="release"
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      exit 1
      ;;
  esac
  shift
done

case "$(uname -s)" in
  Darwin | Linux)
    ;;
  *)
    echo "install-local.sh supports macOS and Linux." >&2
    exit 1
    ;;
esac

require_command cargo
require_command cp
require_command chmod
require_command mkdir

SCRIPT_DIR=$(
  CDPATH= cd -- "$(dirname -- "$0")"
  pwd
)
REPO_ROOT=$(
  CDPATH= cd -- "$SCRIPT_DIR/../.."
  pwd
)
WORKSPACE_DIR="$REPO_ROOT/codex-rs"
TARGET_DIR="$WORKSPACE_DIR/target"

if [ ! -d "$WORKSPACE_DIR" ]; then
  echo "Could not find the codex-rs workspace at $WORKSPACE_DIR" >&2
  exit 1
fi

if ! command -v rg >/dev/null 2>&1; then
  warn "ripgrep (rg) is not installed or not on PATH. This local source install only installs the codex binary."
fi

step "Building Codex CLI from the current checkout ($BUILD_PROFILE)"
if [ "$BUILD_PROFILE" = "release" ]; then
  (
    cd "$WORKSPACE_DIR"
    CARGO_TARGET_DIR="$TARGET_DIR" cargo build -p codex-cli --bin codex --release
  )
else
  (
    cd "$WORKSPACE_DIR"
    CARGO_TARGET_DIR="$TARGET_DIR" cargo build -p codex-cli --bin codex
  )
fi

BINARY_PATH="$TARGET_DIR/$BUILD_PROFILE/codex"
if [ ! -x "$BINARY_PATH" ]; then
  echo "Built binary not found at $BINARY_PATH" >&2
  exit 1
fi

step "Installing to $INSTALL_DIR"
mkdir -p "$INSTALL_DIR"
cp "$BINARY_PATH" "$INSTALL_DIR/codex"
chmod 0755 "$INSTALL_DIR/codex"

add_to_path

case "$path_action" in
  added)
    step "PATH updated for future shells in $path_profile"
    step "Run now: export PATH=\"$INSTALL_DIR:\$PATH\" && codex"
    step "Or open a new terminal and run: codex"
    ;;
  configured)
    step "PATH is already configured for future shells in $path_profile"
    step "Run now: export PATH=\"$INSTALL_DIR:\$PATH\" && codex"
    step "Or open a new terminal and run: codex"
    ;;
  *)
    step "$INSTALL_DIR is already on PATH"
    step "Run: codex"
    ;;
esac

printf 'Codex CLI (%s build) installed successfully.\n' "$BUILD_PROFILE"
