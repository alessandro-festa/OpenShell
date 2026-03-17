#!/bin/sh
# SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0
#
# Install the OpenShell CLI binary.
#
# Usage:
#   curl -LsSf https://raw.githubusercontent.com/NVIDIA/OpenShell/main/install.sh | sh
#
# Or run directly:
#   ./install.sh
#
# Environment variables:
#   OPENSHELL_VERSION     - Release tag to install (default: latest tagged release)
#   OPENSHELL_INSTALL_DIR - Directory to install into (default: ~/.local/bin)
#
# CLI flags:
#   --help            - Print usage information
#   --no-modify-path  - Skip printing PATH setup guidance
#
set -eu

APP_NAME="openshell"
REPO="NVIDIA/OpenShell"
GITHUB_URL="https://github.com/${REPO}"
NO_MODIFY_PATH=0

# ---------------------------------------------------------------------------
# Logging
# ---------------------------------------------------------------------------

info() {
  printf '%s: %s\n' "$APP_NAME" "$*" >&2
}

warn() {
  printf '%s: warning: %s\n' "$APP_NAME" "$*" >&2
}

error() {
  printf '%s: error: %s\n' "$APP_NAME" "$*" >&2
  exit 1
}

# ---------------------------------------------------------------------------
# Usage
# ---------------------------------------------------------------------------

usage() {
  cat <<EOF
install.sh — Install the OpenShell CLI

USAGE:
    curl -LsSf https://raw.githubusercontent.com/NVIDIA/OpenShell/main/install.sh | sh
    ./install.sh [OPTIONS]

OPTIONS:
    --no-modify-path    Don't print PATH setup guidance
    --help              Print this help message

ENVIRONMENT VARIABLES:
    OPENSHELL_VERSION       Release tag to install (default: latest tagged release)
    OPENSHELL_INSTALL_DIR   Directory to install into (default: ~/.local/bin)

EXAMPLES:
    # Install latest release
    curl -LsSf https://raw.githubusercontent.com/NVIDIA/OpenShell/main/install.sh | sh

    # Install a specific version
    OPENSHELL_VERSION=v0.0.4 curl -LsSf https://raw.githubusercontent.com/NVIDIA/OpenShell/main/install.sh | sh

    # Install to /usr/local/bin
    OPENSHELL_INSTALL_DIR=/usr/local/bin curl -LsSf https://raw.githubusercontent.com/NVIDIA/OpenShell/main/install.sh | sh
EOF
}

# ---------------------------------------------------------------------------
# HTTP helpers — prefer curl, fall back to wget
# ---------------------------------------------------------------------------

has_cmd() {
  command -v "$1" >/dev/null 2>&1
}

check_downloader() {
  if has_cmd curl; then
    return 0
  elif has_cmd wget; then
    return 0
  else
    error "either 'curl' or 'wget' is required to download files"
  fi
}

# Download a URL to a file. Outputs nothing on success.
download() {
  _url="$1"
  _output="$2"

  if has_cmd curl; then
    curl -fLsS --retry 3 -o "$_output" "$_url"
  elif has_cmd wget; then
    wget -q --tries=3 -O "$_output" "$_url"
  fi
}

# Follow a URL and print the final resolved URL (for detecting redirect targets).
resolve_redirect() {
  _url="$1"

  if has_cmd curl; then
    curl -fLsS -o /dev/null -w '%{url_effective}' "$_url"
  elif has_cmd wget; then
    # wget --spider follows redirects and prints the final URL
    wget --spider -q --max-redirect=10 "$_url" 2>&1 | grep -oP 'Location: \K\S+' | tail -1
  fi
}

# ---------------------------------------------------------------------------
# Platform detection
# ---------------------------------------------------------------------------

get_os() {
  case "$(uname -s)" in
    Darwin) echo "apple-darwin" ;;
    Linux)  echo "unknown-linux-musl" ;;
    *)      error "unsupported OS: $(uname -s)" ;;
  esac
}

get_arch() {
  case "$(uname -m)" in
    x86_64|amd64)  echo "x86_64" ;;
    aarch64|arm64) echo "aarch64" ;;
    *) error "unsupported architecture: $(uname -m)" ;;
  esac
}

get_target() {
  _arch="$(get_arch)"
  _os="$(get_os)"
  _target="${_arch}-${_os}"

  # Only these targets have published binaries.
  case "$_target" in
    x86_64-unknown-linux-musl|aarch64-unknown-linux-musl|aarch64-apple-darwin) ;;
    x86_64-apple-darwin) error "macOS x86_64 is not supported; use Apple Silicon (aarch64) or Rosetta 2" ;;
    *) error "no prebuilt binary for $_target" ;;
  esac

  echo "$_target"
}

# ---------------------------------------------------------------------------
# Version resolution
# ---------------------------------------------------------------------------

resolve_version() {
  if [ -n "${OPENSHELL_VERSION:-}" ]; then
    echo "$OPENSHELL_VERSION"
    return 0
  fi

  # Resolve "latest" by following the GitHub releases/latest redirect.
  # GitHub redirects /releases/latest -> /releases/tag/<tag>
  info "resolving latest version..."
  _latest_url="${GITHUB_URL}/releases/latest"
  _resolved="$(resolve_redirect "$_latest_url")" || error "failed to resolve latest release from ${_latest_url}"

  # Extract the tag from the resolved URL: .../releases/tag/v0.0.4 -> v0.0.4
  _version="${_resolved##*/}"

  if [ -z "$_version" ] || [ "$_version" = "latest" ]; then
    error "could not determine latest release version (resolved URL: ${_resolved})"
  fi

  echo "$_version"
}

# ---------------------------------------------------------------------------
# Checksum verification
# ---------------------------------------------------------------------------

verify_checksum() {
  _archive="$1"
  _checksums="$2"
  _filename="$3"

  _expected="$(grep "$_filename" "$_checksums" | awk '{print $1}')"

  if [ -z "$_expected" ]; then
    warn "no checksum found for $_filename, skipping verification"
    return 0
  fi

  if has_cmd shasum; then
    echo "$_expected  $_archive" | shasum -a 256 -c --quiet 2>/dev/null
  elif has_cmd sha256sum; then
    echo "$_expected  $_archive" | sha256sum -c --quiet 2>/dev/null
  else
    warn "sha256sum/shasum not found, skipping checksum verification"
    return 0
  fi
}

# ---------------------------------------------------------------------------
# Install location and PATH management
# ---------------------------------------------------------------------------

get_home() {
  if [ -n "${HOME:-}" ]; then
    echo "$HOME"
  elif [ -n "${USER:-}" ]; then
    getent passwd "$USER" | cut -d: -f6
  else
    getent passwd "$(id -un)" | cut -d: -f6
  fi
}

get_default_install_dir() {
  if [ -n "${XDG_BIN_HOME:-}" ]; then
    echo "$XDG_BIN_HOME"
  else
    _home="$(get_home)"
    echo "${_home}/.local/bin"
  fi
}

# Check if a directory is already on PATH.
is_on_path() {
  _dir="$1"
  case ":${PATH}:" in
    *":${_dir}:"*) return 0 ;;
    *)             return 1 ;;
  esac
}

# Print PATH setup guidance without modifying shell config files.
print_path_guidance() {
  _install_dir="$1"
  _current_shell="$(basename "${SHELL:-sh}" 2>/dev/null || echo "sh")"

  if is_on_path "$_install_dir"; then
    return 0
  fi

  echo ""
  info "${APP_NAME} was installed to ${_install_dir}, which is not on your PATH."
  info "add it to your shell config, for example:"
  info ""

  case "$_current_shell" in
    fish)
      info "    fish_add_path \"${_install_dir}\""
      ;;
    *)
      info "    export PATH=\"${_install_dir}:\$PATH\""
      ;;
  esac
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

main() {
  # Parse CLI flags
  for arg in "$@"; do
    case "$arg" in
      --help)
        usage
        exit 0
        ;;
      --no-modify-path)
        NO_MODIFY_PATH=1
        ;;
      *)
        error "unknown option: $arg"
        ;;
    esac
  done

  check_downloader

  _version="$(resolve_version)"
  _target="$(get_target)"
  _filename="${APP_NAME}-${_target}.tar.gz"
  _download_url="${GITHUB_URL}/releases/download/${_version}/${_filename}"
  _checksums_url="${GITHUB_URL}/releases/download/${_version}/${APP_NAME}-checksums-sha256.txt"

  # Determine install directory
  _using_default_dir=0
  if [ -n "${OPENSHELL_INSTALL_DIR:-}" ]; then
    _install_dir="$OPENSHELL_INSTALL_DIR"
  else
    _install_dir="$(get_default_install_dir)"
    _using_default_dir=1
  fi

  info "downloading ${APP_NAME} ${_version} (${_target})..."

  _tmpdir="$(mktemp -d)"
  trap 'rm -rf "$_tmpdir"' EXIT

  if ! download "$_download_url" "${_tmpdir}/${_filename}"; then
    error "failed to download ${_download_url}"
  fi

  # Verify checksum
  info "verifying checksum..."
  if download "$_checksums_url" "${_tmpdir}/checksums.txt"; then
    if ! verify_checksum "${_tmpdir}/${_filename}" "${_tmpdir}/checksums.txt" "$_filename"; then
      error "checksum verification failed for ${_filename}"
    fi
  else
    warn "could not download checksums file, skipping verification"
  fi

  # Extract
  info "extracting..."
  tar -xzf "${_tmpdir}/${_filename}" -C "${_tmpdir}"

  # Install
  mkdir -p "$_install_dir" 2>/dev/null || true

  if [ -w "$_install_dir" ] || mkdir -p "$_install_dir" 2>/dev/null; then
    install -m 755 "${_tmpdir}/${APP_NAME}" "${_install_dir}/${APP_NAME}"
  else
    info "elevated permissions required to install to ${_install_dir}"
    sudo mkdir -p "$_install_dir"
    sudo install -m 755 "${_tmpdir}/${APP_NAME}" "${_install_dir}/${APP_NAME}"
  fi

  _installed_version="$("${_install_dir}/${APP_NAME}" --version 2>/dev/null || echo "${_version}")"
  info "installed ${APP_NAME} ${_installed_version} to ${_install_dir}/${APP_NAME}"

  # Print PATH guidance for default install location
  if [ "$_using_default_dir" = "1" ] && [ "$NO_MODIFY_PATH" = "0" ]; then
    print_path_guidance "$_install_dir"
  fi
}

main "$@"
