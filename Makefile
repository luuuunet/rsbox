.PHONY: help build build-release test fmt clippy clean run doc install-tools audit

help:
	@echo "rsbox Makefile"
	@echo ""
	@echo "Available targets:"
	@echo "  build          - Build debug version"
	@echo "  build-release  - Build optimized release version"
	@echo "  test           - Run all tests"
	@echo "  fmt            - Format code with rustfmt"
	@echo "  clippy         - Run clippy linter"
	@echo "  clean          - Clean build artifacts"
	@echo "  run            - Run with example config"
	@echo "  doc            - Generate documentation"
	@echo "  install-tools  - Install development tools"
	@echo "  audit          - Security audit dependencies"

# Development builds
build:
	cargo build --workspace

build-release:
	cargo build --release -p rsbox --features rsb-protocol/wireguard-tunnel

# Build without WireGuard (smaller binary)
build-minimal:
	cargo build --release -p rsbox --no-default-features

# Testing
test:
	cargo test --workspace --verbose

test-all:
	cargo test --workspace --all-features --verbose

# Code quality
fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

clippy:
	cargo clippy --workspace --all-features -- -D warnings

clippy-fix:
	cargo clippy --workspace --all-features --fix --allow-dirty

# Cleanup
clean:
	cargo clean
	rm -rf target/

# Run
run:
	cargo run -p rsbox -- run -c config.example.json

run-release:
	cargo run --release -p rsbox -- run -c config.example.json

# Documentation
doc:
	cargo doc --workspace --no-deps --open

doc-build:
	cargo doc --workspace --no-deps

# Development tools
install-tools:
	rustup component add rustfmt clippy
	cargo install cargo-audit cargo-outdated cargo-tree

# Security
audit:
	cargo audit

outdated:
	cargo outdated

# CI simulation
ci: fmt-check clippy test
	@echo "✓ All CI checks passed!"

# Build for all platforms (requires cross)
build-cross:
	cross build --release --target x86_64-unknown-linux-gnu -p rsbox
	cross build --release --target aarch64-unknown-linux-gnu -p rsbox
	cross build --release --target x86_64-pc-windows-gnu -p rsbox

# Install locally
install:
	cargo install --path rsbox --features rsb-protocol/wireguard-tunnel

# Benchmarks (if any)
bench:
	cargo bench --workspace
