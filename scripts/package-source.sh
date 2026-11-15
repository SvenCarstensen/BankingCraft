#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
root="$PWD"
version="$(python3 scripts/release-version.py)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
name="BankingCraft-$version-source"
stage="$work/$name"
mkdir -p "$stage/.cargo" "$root/dist"
# Only committed files enter the source release; never include the developer's local vault/config.
git archive HEAD | tar -x -C "$stage"
cargo vendor --locked --versioned-dirs "$stage/vendor" > "$stage/.cargo/config.toml"
python3 - "$stage/.cargo/config.toml" "$stage/vendor" <<'PY'
from pathlib import Path
import sys
config = Path(sys.argv[1])
config.write_text(config.read_text().replace(sys.argv[2], "vendor"))
PY
git rev-parse HEAD > "$stage/SOURCE_COMMIT.txt"
rustc --version --verbose > "$stage/SOURCE_TOOLCHAIN.txt"
(cd "$stage" && cargo metadata --offline --locked --format-version 1 > /dev/null)
tar -czf "$root/dist/$name.tar.gz" -C "$work" "$name"
