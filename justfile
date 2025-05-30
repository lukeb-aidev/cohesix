# Quick-tasks for Cohesix developers

# ---- primary CI replica ----
ci: fmt-check clippy test build

build:
    cargo check --all-targets

release-build:
    cargo build --release --all-targets

test:
    cargo test --all-targets --verbose

clippy:
    cargo clippy --all-targets -- -D warnings

fmt:
    cargo fmt

fmt-check:
    cargo fmt -- --check

# ---- batch integration helper ----
# Usage: just integrate-batch BATCH=cc5
integrate-batch batch:
    git checkout -b "batch/{{batch}}-integration"
    tar -xzf "{{batch}}.tar.gz" -C .
    just ci
    git add -A
    git commit -m "Integrate {{batch}}"
    @echo "✨ Batch {{batch}} integrated & locally green."
