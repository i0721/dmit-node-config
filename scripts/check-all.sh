#!/bin/bash
set -euo pipefail
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo run --quiet --locked -- schema > /tmp/nodectl-schema.json
diff -u schemas/node.schema.json /tmp/nodectl-schema.json
python3 scripts/check-docs.py
sh -n install.sh
for script in scripts/*.sh; do bash -n "$script"; done
python3 scripts/check-workflows.py
