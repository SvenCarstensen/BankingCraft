#!/bin/sh
# Offline checks only: synthetic data, mocked banks and temporary test vaults.
set -eu
cd -- "$(dirname -- "$0")/.."
python3 scripts/update-licenses.py --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
