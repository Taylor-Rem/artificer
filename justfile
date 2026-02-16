# Run engine with hot reload
dev-engine:
    cargo watch \
      --why \
      --clear \
      --ignore '*.db' \
      --ignore 'target/*' \
      -w crates/engine \
      -w crates/tools \
      -x 'run --bin artificer'

# Run envoy with hot reload
dev-envoy:
    cargo watch \
      --why \
      --clear \
      --ignore '*.db' \
      --ignore 'target/*' \
      -w crates/envoy \
      -w crates/tools \
      -x 'run --bin envoy'

# Build everything
build:
    cargo build

# Run tests
test:
    cargo test

# Clean and rebuild
clean:
    cargo clean
    cargo build