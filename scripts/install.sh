#!/bin/sh
set -eu

REPO="${PGSANDBOX_REPO:-LVTD-LLC/pgsandbox-mcp}"
GITHUB_BASE_URL="${PGSANDBOX_GITHUB_BASE_URL:-https://github.com}"
GITHUB_API_URL="${PGSANDBOX_GITHUB_API_URL:-https://api.github.com}"
BINARY_NAME="pgsandbox-mcp"

if [ -n "${PGSANDBOX_INSTALL_DIR:-}" ]; then
  INSTALL_DIR="$PGSANDBOX_INSTALL_DIR"
elif [ -n "${HOME:-}" ]; then
  INSTALL_DIR="$HOME/.local/bin"
else
  INSTALL_DIR="/usr/local/bin"
fi

say() {
  printf '%s\n' "$*"
}

warn() {
  printf 'warning: %s\n' "$*" >&2
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

have() {
  command -v "$1" >/dev/null 2>&1
}

http_get() {
  url="$1"
  if have curl; then
    curl -fsSL "$url"
  elif have wget; then
    wget -qO- "$url"
  else
    die "curl or wget is required"
  fi
}

download() {
  url="$1"
  output="$2"
  if have curl; then
    curl -fL --retry 3 --proto '=https' --tlsv1.2 -o "$output" "$url"
  elif have wget; then
    wget -q -O "$output" "$url"
  else
    die "curl or wget is required"
  fi
}

latest_version() {
  tag="$(
    http_get "$GITHUB_API_URL/repos/$REPO/releases/latest" \
      | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
      | head -n 1
  )"
  [ -n "$tag" ] || die "could not resolve latest release for $REPO"
  printf '%s' "${tag#v}"
}

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$arch" in
    x86_64 | amd64) arch="x86_64" ;;
    arm64 | aarch64) arch="aarch64" ;;
    *) die "unsupported CPU architecture: $arch" ;;
  esac

  case "$os" in
    Darwin) printf '%s-apple-darwin' "$arch" ;;
    Linux) printf '%s-unknown-linux-gnu' "$arch" ;;
    *) die "unsupported operating system: $os" ;;
  esac
}

sha256_file() {
  file="$1"
  if have shasum; then
    shasum -a 256 "$file" | awk '{print $1}'
  elif have sha256sum; then
    sha256sum "$file" | awk '{print $1}'
  else
    return 1
  fi
}

verify_checksum() {
  checksum_file="$1"
  archive="$2"
  asset_name="$3"

  expected="$(
    grep "  $asset_name\$" "$checksum_file" | awk '{print $1}' | head -n 1 || true
  )"

  if [ -z "$expected" ]; then
    warn "no checksum entry found for $asset_name"
    return 0
  fi

  actual="$(sha256_file "$archive" || true)"
  if [ -z "$actual" ]; then
    warn "shasum or sha256sum is required to verify checksums"
    return 0
  fi

  [ "$expected" = "$actual" ] || die "checksum mismatch for $asset_name"
  say "Verified checksum for $asset_name"
}

VERSION="${PGSANDBOX_VERSION:-}"
if [ -z "$VERSION" ]; then
  VERSION="$(latest_version)"
fi
VERSION="${VERSION#v}"
RELEASE_TAG="v$VERSION"
TARGET="${PGSANDBOX_TARGET:-$(detect_target)}"
ASSET="pgsandbox-mcp-$VERSION-$TARGET.tar.gz"
CHECKSUMS="pgsandbox-mcp-$VERSION-checksums.txt"
DOWNLOAD_URL="$GITHUB_BASE_URL/$REPO/releases/download/$RELEASE_TAG/$ASSET"
CHECKSUM_URL="$GITHUB_BASE_URL/$REPO/releases/download/$RELEASE_TAG/$CHECKSUMS"

tmpdir="$(mktemp -d 2>/dev/null || mktemp -d -t pgsandbox)"
trap 'rm -rf "$tmpdir"' EXIT INT HUP TERM

archive="$tmpdir/$ASSET"
checksum_file="$tmpdir/$CHECKSUMS"

say "Installing $BINARY_NAME $VERSION for $TARGET"
say "Downloading $DOWNLOAD_URL"
if ! download "$DOWNLOAD_URL" "$archive"; then
  die "could not download $ASSET. Check that release $RELEASE_TAG contains a $TARGET asset."
fi

if [ "${PGSANDBOX_SKIP_CHECKSUM:-0}" = "1" ]; then
  warn "skipping checksum verification"
elif download "$CHECKSUM_URL" "$checksum_file"; then
  verify_checksum "$checksum_file" "$archive" "$ASSET"
else
  warn "could not download $CHECKSUMS; installing without checksum verification"
fi

tar -xzf "$archive" -C "$tmpdir"
[ -f "$tmpdir/$BINARY_NAME" ] || die "archive did not contain $BINARY_NAME"

mkdir -p "$INSTALL_DIR"
if have install; then
  install -m 0755 "$tmpdir/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
else
  cp "$tmpdir/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
  chmod 0755 "$INSTALL_DIR/$BINARY_NAME"
fi

say "Installed $INSTALL_DIR/$BINARY_NAME"
"$INSTALL_DIR/$BINARY_NAME" --version || true

case ":${PATH:-}:" in
  *":$INSTALL_DIR:"*) ;;
  *) warn "$INSTALL_DIR is not on PATH" ;;
esac

say "Next: run \`pgsandbox-mcp setup --client codex --admin-url \"\$PGSANDBOX_ADMIN_DATABASE_URL\"\`"
