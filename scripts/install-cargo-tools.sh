#!/usr/bin/env bash

# Exit immediately if a command exits with a non-zero status,
# or if an uninitialized variable is used.
set -euo pipefail

// check for 'cargo' comand
command -v cargo >/dev/null 2>&1 || {
  echo "Error: cargo is not installed or not on PATH" >&2
  exit 1
}

if ! cargo install --list | grep -q '^sqlx-cli '; then
  cargo install sqlx-cli --locked
fi

if ! cargo install --list | grep -q '^cargo-llvm-cov '; then
  cargo install cargo-llvm-cov --locked
fi

rustup component add llvm-tools-preview