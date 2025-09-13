#!/usr/bin/env bash
set -e

MIDEN_VM_REPO_URL="https://github.com/0xPolygonMiden/miden-vm.git"
CARGO_BIN_DIR="${CARGO_HOME:-$HOME/.cargo}/bin"
MIDEN_VERSION=${MIDEN_VERSION:-v0.17.1}
RUST_TOOLCHAIN_VERSION=${RUST_TOOLCHAIN_VERSION:-1.88.0}

echo "Installing Miden VM CLI (${MIDEN_VERSION})..."

# Check for required dependencies (git, rustup, cargo).
echo "Checking prerequisites..."
for tool in git rustup cargo; do
    if ! command -v "$tool" &>/dev/null; then
        echo "Error: Required tool '${tool}' is not installed." >&2
        exit 1
    fi
done

# Ensure the correct Rust toolchain is installed.
echo "Installing Rust toolchain ${RUST_TOOLCHAIN_VERSION}..."
rustup toolchain install "${RUST_TOOLCHAIN_VERSION}" --profile minimal

# Clone, build, and install the Miden VM from source.
echo "Building from source..."
tmp_dir=$(mktemp -d)
trap 'rm -rf -- "$tmp_dir"' EXIT
git clone --quiet --depth 1 --branch "${MIDEN_VERSION}" "${MIDEN_VM_REPO_URL}" "$tmp_dir"
cargo "+${RUST_TOOLCHAIN_VERSION}" install --path "$tmp_dir/miden-vm" --features executable

# Verify the installation.
echo "Verifying installation"
export PATH="${CARGO_BIN_DIR}:$PATH"
if ! miden-vm --version &>/dev/null; then
    echo "Error: 'miden-vm' command failed. Ensure '${CARGO_BIN_DIR}' is in your PATH." >&2
    exit 1
fi

echo
echo "Miden VM installation successful."
echo "To use it, add the following to your shell profile (e.g., ~/.bashrc):"
echo
echo "  export PATH=\"${CARGO_BIN_DIR}:\$PATH\""
echo
echo "Then restart your terminal or source the profile file."
