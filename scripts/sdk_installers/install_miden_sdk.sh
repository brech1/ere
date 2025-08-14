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

# Prerequisites
ensure_tool_installed "git" "to clone git dependencies"
ensure_tool_installed "rustup" "for managing Rust toolchains"
ensure_tool_installed "cargo" "to build Rust packages"

# Verify that the ere-miden crate can be built
if [ -d "crates/ere-miden" ]; then
    echo "Verifying ere-miden crate can build..."
    cargo +stable check -p ere-miden || cargo check -p ere-miden
    echo "ere-miden crate verification successful."
else
    echo "Warning: crates/ere-miden directory not found. This script should be run from the ere workspace root."
    echo "         ere-miden verification skipped."
fi
