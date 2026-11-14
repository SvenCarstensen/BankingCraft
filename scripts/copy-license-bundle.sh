#!/bin/sh
# Copy the reviewed notices and editable bank data into an application's resources.
set -eu
destination=${1:?Usage: copy-license-bundle.sh /absolute/path/to/resources}
case "$destination" in
    /*) ;;
    *) echo "The resources path must be absolute." >&2; exit 1 ;;
esac
cd -- "$(dirname -- "$0")/.."
python3 scripts/update-licenses.py --check
mkdir -p "$destination/app/data" "$destination/docs"
cp LICENSE THIRD_PARTY_NOTICES.md "$destination/"
cp -R licenses "$destination/"
cp app/data/banks.tsv "$destination/app/data/banks.tsv"
cp docs/bank-compatibility.md "$destination/docs/bank-compatibility.md"
