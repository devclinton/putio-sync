.PHONY: help build test clean fmt lint ci install run-cli dev release check doc

# Default target - show help
.DEFAULT_GOAL := help

# Detect OS for conditional commands
ifeq ($(OS),Windows_NT)
    DETECTED_OS := Windows
    CI_SCRIPT := powershell -ExecutionPolicy Bypass -File scripts/ci-local.ps1
else
    DETECTED_OS := $(shell uname -s)
    CI_SCRIPT := bash scripts/ci-local.sh
endif

help: ## Show this help message
	@echo 'Usage: make [target]'
	@echo ''
	@echo 'Available targets:'
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@echo ''
	@echo 'Development workflow:'
	@echo '  1. make dev      - Run in development mode with auto-reload'
	@echo '  2. make test     - Run all tests'
	@echo '  3. make ci       - Run all CI checks locally'
	@echo '  4. make build    - Build release binary'
	@echo ''
	@echo 'Detected OS: $(DETECTED_OS)'

build: ## Build the project in release mode
	@echo "Building release binary..."
	cargo build --release

build-debug: ## Build the project in debug mode
	@echo "Building debug binary..."
	cargo build

test: ## Run all tests
	@echo "Running tests..."
	cargo test --all-features --verbose

test-unit: ## Run only unit tests
	@echo "Running unit tests..."
	cargo test --lib --all-features

test-integration: ## Run only integration tests
	@echo "Running integration tests..."
	cargo test --test '*' --all-features

clean: ## Clean build artifacts
	@echo "Cleaning build artifacts..."
	cargo clean

fmt: ## Format code with rustfmt
	@echo "Formatting code..."
	cargo fmt --all

fmt-check: ## Check code formatting without modifying files
	@echo "Checking code formatting..."
	cargo fmt --all -- --check

lint: ## Run clippy lints
	@echo "Running clippy..."
	cargo clippy --all-targets --all-features -- -D warnings

lint-fix: ## Run clippy with automatic fixes
	@echo "Running clippy with fixes..."
	cargo clippy --all-targets --all-features --fix

ci: ## Run all CI checks locally (format, lint, test, build)
	@echo "Running local CI checks..."
	@$(CI_SCRIPT)

check: ## Quick check that the code compiles
	@echo "Checking compilation..."
	cargo check --all-targets --all-features

install: ## Install the CLI binary to ~/.cargo/bin
	@echo "Installing putio-sync..."
	cargo install --path putio-sync-cli

run-cli: ## Run the CLI in development mode
	@echo "Running CLI..."
	cargo run --bin putio-sync -- $(ARGS)

dev: ## Run in development mode (use with ARGS="command args")
	@echo "Running in development mode..."
	cargo run --bin putio-sync -- $(ARGS)

release: ## Build optimized release binaries
	@echo "Building release binaries..."
	cargo build --release --all-features
	@echo "Binaries available in: target/release/"

watch: ## Watch for changes and run tests automatically
	@echo "Watching for changes..."
	@command -v cargo-watch >/dev/null 2>&1 || { echo "cargo-watch not installed. Run: cargo install cargo-watch"; exit 1; }
	cargo watch -x test

doc: ## Generate and open documentation
	@echo "Generating documentation..."
	cargo doc --all-features --no-deps --open

doc-build: ## Generate documentation without opening
	@echo "Generating documentation..."
	cargo doc --all-features --no-deps

coverage: ## Generate test coverage report (requires cargo-tarpaulin)
	@echo "Generating coverage report..."
	@command -v cargo-tarpaulin >/dev/null 2>&1 || { echo "cargo-tarpaulin not installed. Run: cargo install cargo-tarpaulin"; exit 1; }
	cargo tarpaulin --all-features --workspace --out Html --output-dir coverage

bench: ## Run benchmarks (if any)
	@echo "Running benchmarks..."
	cargo bench

update: ## Update dependencies
	@echo "Updating dependencies..."
	cargo update

audit: ## Check for security vulnerabilities in dependencies
	@echo "Auditing dependencies..."
	@command -v cargo-audit >/dev/null 2>&1 || { echo "cargo-audit not installed. Run: cargo install cargo-audit"; exit 1; }
	cargo audit

bloat: ## Analyze binary size (requires cargo-bloat)
	@echo "Analyzing binary size..."
	@command -v cargo-bloat >/dev/null 2>&1 || { echo "cargo-bloat not installed. Run: cargo install cargo-bloat"; exit 1; }
	cargo bloat --release -n 20

tree: ## Show dependency tree
	@echo "Showing dependency tree..."
	cargo tree

setup: ## Install development tools
	@echo "Installing development tools..."
	rustup component add rustfmt clippy
	@echo "Optional tools (install if needed):"
	@echo "  cargo install cargo-watch    # For 'make watch'"
	@echo "  cargo install cargo-tarpaulin # For 'make coverage'"
	@echo "  cargo install cargo-audit     # For 'make audit'"
	@echo "  cargo install cargo-bloat     # For 'make bloat'"

login: ## Run login command (use with ARGS="--token YOUR_TOKEN")
	@echo "Running login command..."
	cargo run --bin putio-sync -- login $(ARGS)

list: ## Run list command (use with ARGS="/path/to/folder")
	@echo "Running list command..."
	cargo run --bin putio-sync -- list $(ARGS)
