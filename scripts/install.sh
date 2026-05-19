#!/usr/bin/env sh
# agenttop install script — downloads the latest GitHub release for your
# OS/arch, verifies its SHA256 against the published checksum, and drops
# the binary into ~/.local/bin (or $AGENTTOP_INSTALL_DIR if set).
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/tech4242/agenttop/main/scripts/install.sh | sh
#
# Optional env vars:
#   AGENTTOP_INSTALL_DIR   — install destination (default: $HOME/.local/bin)
#   AGENTTOP_VERSION       — pin to a specific tag (default: latest release)

set -eu

REPO="tech4242/agenttop"
INSTALL_DIR="${AGENTTOP_INSTALL_DIR:-$HOME/.local/bin}"

bold() { printf '\033[1m%s\033[0m\n' "$1"; }
red()  { printf '\033[31m%s\033[0m\n' "$1" >&2; }
green(){ printf '\033[32m%s\033[0m\n' "$1"; }

bold "agenttop installer"

# ---- Detect OS + arch ----
os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
    Darwin)  os_short="darwin" ;;
    Linux)   os_short="linux"  ;;
    *) red "Unsupported OS: $os (only macOS and Linux are supported)"; exit 1 ;;
esac

case "$arch" in
    x86_64|amd64)  arch_short="x86_64" ;;
    arm64|aarch64) arch_short="arm64"  ;;
    *) red "Unsupported architecture: $arch"; exit 1 ;;
esac

# The release tarball naming convention from .github/workflows/release.yml.
# macOS uses arm64/x86_64; Linux uses x86_64/aarch64.
if [ "$os_short" = "linux" ] && [ "$arch_short" = "arm64" ]; then
    arch_short="aarch64"
fi

artifact="agenttop-${os_short}-${arch_short}.tar.gz"
echo "Platform: $os $arch  →  $artifact"

# ---- Find required tools ----
for tool in curl tar uname; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        red "Required tool not found: $tool"
        exit 1
    fi
done

if command -v sha256sum >/dev/null 2>&1; then
    SHA_CMD="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
    SHA_CMD="shasum -a 256"
else
    red "Need sha256sum or shasum to verify the download — please install one."
    exit 1
fi

# ---- Resolve version ----
if [ -n "${AGENTTOP_VERSION:-}" ]; then
    version="$AGENTTOP_VERSION"
    echo "Using pinned version: $version"
else
    echo "Resolving latest release..."
    # Follow redirects on /releases/latest and grab the tag from the final URL.
    version="$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
        "https://github.com/${REPO}/releases/latest" \
        | sed 's#.*/tag/##')"
    if [ -z "$version" ]; then
        red "Failed to resolve latest release tag."
        exit 1
    fi
    echo "Latest: $version"
fi

# ---- Download tarball + checksum ----
base_url="https://github.com/${REPO}/releases/download/${version}"
tarball_url="${base_url}/${artifact}"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

echo "Downloading $tarball_url ..."
if ! curl -fsSL -o "${tmpdir}/${artifact}" "$tarball_url"; then
    red "Download failed. The release may not have a build for $artifact."
    red "Check: https://github.com/${REPO}/releases/${version}"
    exit 1
fi

# Try to fetch a sibling .sha256 file. Not all release workflows publish
# one yet — when absent, fall back to computing & printing the hash so the
# user can eyeball it. (Hardening: future release.yml should always emit
# .sha256 files alongside each tarball.)
sha_url="${tarball_url}.sha256"
if curl -fsSL -o "${tmpdir}/${artifact}.sha256" "$sha_url" 2>/dev/null; then
    expected="$(cut -d' ' -f1 "${tmpdir}/${artifact}.sha256")"
    actual="$(${SHA_CMD} "${tmpdir}/${artifact}" | cut -d' ' -f1)"
    if [ "$expected" != "$actual" ]; then
        red "SHA256 mismatch — refusing to install."
        red "  expected: $expected"
        red "  got:      $actual"
        exit 1
    fi
    green "SHA256 verified."
else
    echo "(No published .sha256 for $version yet — recording the hash for your records:)"
    ${SHA_CMD} "${tmpdir}/${artifact}"
fi

# ---- Extract + install ----
tar -xzf "${tmpdir}/${artifact}" -C "$tmpdir"
if [ ! -f "${tmpdir}/agenttop" ]; then
    red "Tarball did not contain 'agenttop' at the root."
    exit 1
fi

mkdir -p "$INSTALL_DIR"
mv "${tmpdir}/agenttop" "${INSTALL_DIR}/agenttop"
chmod +x "${INSTALL_DIR}/agenttop"

green "Installed agenttop to ${INSTALL_DIR}/agenttop"

# ---- PATH hint ----
case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
        echo
        bold "One more step — add ${INSTALL_DIR} to your PATH:"
        echo "  echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.bashrc"
        echo "  # or your shell's equivalent (.zshrc, config.fish, etc.)"
        echo
        ;;
esac

echo
bold "Next: run \`agenttop\`"
echo "Need help with a specific provider? Try \`agenttop --setup <provider>\`."
