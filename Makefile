.PHONY: build build-release build-server build-client test up run clean help

# Default target
help:
	@echo "VectorShell Makefile"
	@echo ""
	@echo "Usage:"
	@echo "  make build          Build debug binaries (server + client)"
	@echo "  make build-release  Build release binaries"
	@echo "  make build-server   Build only the server"
	@echo "  make build-client   Build only the client"
	@echo "  make test           Run all Rust tests"
	@echo "  make test-watch     Run tests in watch mode (requires cargo-watch)"
	@echo "  make up             Run the server with default config"
	@echo "  make clean          Clean build artifacts"
	@echo "  make web-install    Install dashboard dependencies"
	@echo "  make web-dev        Start dashboard Vite dev server"
	@echo "  make web-build      Build dashboard for production"
	@echo "  make lint           Lint the Rust workspace"
	@echo "  make gen-client     Generate client binary (release)"
	@echo "  make gen-client TARGET=linux-arm64  Generate for specific target"

# Build commands
build: web-build
	cargo build --release

build-release:
	cargo build --release

build-server:
	cargo build -p vectorshell-server

build-client:
	cargo build -p vectorshell-client

gen-client:
	cargo run -p vectorshell-server -- --config config/config.toml generate-client $(if $(TARGET),--target $(TARGET),)

# Test commands
test:
	cargo test

test-server:
	cargo test -p vectorshell-server

test-client:
	cargo test -p vectorshell-client

test-shared:
	cargo test -p shared

test-single:
	$(error Usage: make test-single TEST=name, e.g. make test-single TEST=register_message_roundtrip)

# Run commands
up:
	cargo run -p vectorshell-server -- --config config/config.toml

run:
	cargo run -p vectorshell-server -- --config config/config.toml $(ARGS)

# Webapp commands
web-install:
	npm install --prefix dashboard

web-dev:
	npm run --prefix dashboard dev

web-build: web-install
	npm run --prefix dashboard build

web-preview:
	npm run --prefix dashboard preview

web-lint:
	npm run --prefix dashboard lint

# Code quality
lint:
	cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings

fmt:
	cargo fmt --all

# Cleanup
clean:
	cargo clean
	rm -rf dashboard/dist
	rm -rf dashboard/node_modules/.vite

# Dependencies
check:
	cargo check --all
