#!/bin/bash
set -euo pipefail
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo run --quiet --locked --bin nodectl -- schema > /tmp/nodectl-schema.json
diff -u schemas/node.schema.json /tmp/nodectl-schema.json
cargo run --quiet --locked --bin repo-tools -- check-docs
sh -n install.sh
for script in scripts/*.sh; do bash -n "$script"; done
cargo run --quiet --locked --bin repo-tools -- check-workflows
cargo run --quiet --locked --bin repo-tools -- check-native
