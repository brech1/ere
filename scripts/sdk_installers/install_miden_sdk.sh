#!/bin/bash
set -e

# --- Utility functions (duplicated) ---
# Checks if a tool is installed and available in PATH.
is_tool_installed() {
    command -v "$1" &> /dev/null
}

# Ensures a tool is installed. Exits with an error if not.
ensure_tool_installed() {
    local tool_name="$1"
    local purpose_message="$2"
    if ! is_tool_installed "${tool_name}"; then
        echo "Error: Required tool '${tool_name}' could not be found." >&2
        if [ -n "${purpose_message}" ]; then
            echo "       It is needed ${purpose_message}." >&2
        fi
        echo "       Please install it first and ensure it is in your PATH." >&2
        exit 1
    fi
}
# --- End of Utility functions ---

echo "Setting up Miden development environment..."

# 1. Prerequisites
ensure_tool_installed "git" "to install cargo dependencies from git repositories"
ensure_tool_installed "rustup" "for managing Rust toolchains"
ensure_tool_installed "cargo" "to build and install Rust packages"

# 2. Define Miden-specific versions
# Using a known compatible toolchain and client version for stability.
MIDEN_CLI_VERSION_TAG="v0.17.0"

# 4. Install the Miden Client CLI using the specified toolchain and version tag
echo "Installing Miden Client (version ${MIDEN_CLI_VERSION_TAG}) from GitHub repository (0xPolygonMiden/miden-vm)..."
cargo "+${MIDEN_TOOLCHAIN_VERSION}" install --git https://github.com/0xPolygonMiden/miden-vm --tag "${MIDEN_CLI_VERSION_TAG}" miden-client

# 5. Verify the Miden Client installation
echo "Verifying Miden Client installation..."
if miden-client --version; then
    echo "Miden Client installation verified successfully."
else
    echo "Error: 'miden-client --version' failed. The Miden Client might not have installed correctly." >&2
    echo "       Ensure that your Cargo binary path (${HOME}/.cargo/bin) is included in your shell's PATH." >&2
    exit 1
fi
