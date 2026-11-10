.PHONY: help build test clean fmt lint ci install run-cli dev release check doc i18n i18n-translate \
	dist-linux dist-macos docker-dhi \
	docker-build docker-test docker-run docker-push docker-clean \
	build-deb build-snap build-flatpak packages \
	ci-docker workflow-ci workflow-docker

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
	@echo 'Common Development Workflow:'
	@echo '  1. make dev          - Run in development mode'
	@echo '  2. make test         - Run all tests'
	@echo '  3. make ci           - Run all CI checks locally'
	@echo '  4. make build        - Build release binary'
	@echo '  5. make docker-build - Build Docker image'
	@echo ''
	@echo '━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━'
	@echo 'Build & Development Targets:'
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / && !/^(docker|build-(deb|snap|flatpak)|packages|workflow|ci-docker|all-|clean-all)/ {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@echo ''
	@echo 'Docker Targets:'
	@awk 'BEGIN {FS = ":.*?## "} /^docker-[a-zA-Z_-]+:.*?## / {printf "  \033[36m%-25s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@echo '  Recommended for development: make docker-build-fast'
	@echo '  Recommended for CI/CD: make docker-build-cache'
	@echo ''
	@echo 'Package Build Targets:'
	@awk 'BEGIN {FS = ":.*?## "} /^(build-(deb|snap|flatpak)|packages)[a-zA-Z_-]*:.*?## / {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@echo ''
	@echo 'Workflow & CI Targets:'
	@awk 'BEGIN {FS = ":.*?## "} /^(workflow|ci-docker)[a-zA-Z_-]*:.*?## / {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@echo ''
	@echo 'Combined Targets:'
	@awk 'BEGIN {FS = ":.*?## "} /^(all-|clean-all)[a-zA-Z_-]*:.*?## / {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@echo ''
	@echo '━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━'
	@echo 'Environment:'
	@echo '  Detected OS: $(DETECTED_OS)'
	@echo ''
	@echo 'Variables (override with VAR=value):'
	@echo '  DOCKER_IMAGE     - Docker image name (default: $(DOCKER_IMAGE))'
	@echo '  DOCKER_TAG       - Docker image tag (default: $(DOCKER_TAG))'
	@echo '  PACKAGE_VERSION  - Package version (default: $(PACKAGE_VERSION))'
	@echo '  DEB_ARCH         - Debian architecture (default: $(DEB_ARCH))'
	@echo ''

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

i18n: ## Refresh the web UI's English catalog from its strings (feature 051)
	node scripts/i18n-extract.mjs

i18n-translate: i18n ## Translate the web UI's missing strings (needs atranslate-cli and ATRANSLATE_URL)
	node scripts/i18n-translate.mjs $(LANGS)

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

# ============================================================================
# Docker Targets
# ============================================================================

DOCKER_IMAGE ?= putio-sync
DOCKER_TAG ?= latest
DOCKER_REGISTRY ?= ghcr.io
GIT_SHA ?= $(shell git rev-parse --short HEAD 2>/dev/null || echo unknown)
DOCKER_FULL_IMAGE ?= $(DOCKER_REGISTRY)/$(DOCKER_IMAGE):$(DOCKER_TAG)

docker-build: ## Build Docker image with BuildKit caching (Alpine + musl)
	@echo "Building Docker image with BuildKit caching: $(DOCKER_IMAGE):$(DOCKER_TAG)"
	env DOCKER_BUILDKIT=1 docker build \
		--cache-from=$(DOCKER_IMAGE):cache \
		--build-arg BUILDKIT_INLINE_CACHE=1 \
		--build-arg PUTIO_SYNC_GIT_SHA=$(GIT_SHA) \
		-t $(DOCKER_IMAGE):$(DOCKER_TAG) .
	@echo "Image built successfully: $(DOCKER_IMAGE):$(DOCKER_TAG)"

docker-build-cache: ## Build Docker image with external cache support (registry-based)
	@echo "Building Docker image with registry cache: $(DOCKER_IMAGE):$(DOCKER_TAG)"
	env DOCKER_BUILDKIT=1 docker buildx build \
		--cache-from=type=registry,ref=$(DOCKER_REGISTRY)/$(DOCKER_IMAGE):cache \
		--cache-to=type=registry,ref=$(DOCKER_REGISTRY)/$(DOCKER_IMAGE):cache,mode=max \
		--load \
		--build-arg PUTIO_SYNC_GIT_SHA=$(GIT_SHA) \
		-t $(DOCKER_IMAGE):$(DOCKER_TAG) .
	@echo "Image built successfully with registry cache: $(DOCKER_IMAGE):$(DOCKER_TAG)"

docker-build-local-cache: ## Build Docker image with local cache (requires buildx with docker-container driver)
	@echo "Setting up buildx builder for local caching..."
	@docker buildx inspect cache-builder >/dev/null 2>&1 || \
		docker buildx create --name cache-builder --driver docker-container --use
	@echo "Building Docker image with local BuildKit cache: $(DOCKER_IMAGE):$(DOCKER_TAG)"
	env DOCKER_BUILDKIT=1 docker buildx build \
		--cache-from=type=local,src=/tmp/docker-cache \
		--cache-to=type=local,dest=/tmp/docker-cache,mode=max \
		--load \
		--build-arg PUTIO_SYNC_GIT_SHA=$(GIT_SHA) \
		-t $(DOCKER_IMAGE):$(DOCKER_TAG) .
	@echo "Image built successfully with local cache: $(DOCKER_IMAGE):$(DOCKER_TAG)"

docker-build-multiarch: ## Build multi-architecture Docker image (amd64, arm64)
	@echo "Building multi-architecture Docker image with caching..."
	docker buildx build --platform linux/amd64,linux/arm64 \
		--cache-from=type=registry,ref=$(DOCKER_REGISTRY)/$(DOCKER_IMAGE):cache \
		--cache-to=type=registry,ref=$(DOCKER_REGISTRY)/$(DOCKER_IMAGE):cache,mode=max \
		--build-arg PUTIO_SYNC_GIT_SHA=$(GIT_SHA) \
		-t $(DOCKER_IMAGE):$(DOCKER_TAG) .

docker-test: docker-build ## Test Docker image
	@echo "Testing Docker image..."
	docker run --rm $(DOCKER_IMAGE):$(DOCKER_TAG) putio-sync --help
	docker run --rm $(DOCKER_IMAGE):$(DOCKER_TAG) putio-sync --version
	@echo "Docker image test passed!"

docker-run: ## Run Docker container (use with ARGS="command args")
	@echo "Running Docker container..."
	docker run --rm -it \
		-v $(PWD)/config:/config \
		-v $(PWD)/downloads:/downloads \
		$(DOCKER_IMAGE):$(DOCKER_TAG) $(ARGS)

docker-shell: ## Open shell in Docker container
	@echo "Opening shell in Docker container..."
	docker run --rm -it \
		-v $(PWD)/config:/config \
		-v $(PWD)/downloads:/downloads \
		--entrypoint /bin/sh \
		$(DOCKER_IMAGE):$(DOCKER_TAG)

docker-push: docker-build ## Push Docker image to registry
	@echo "Pushing Docker image to $(DOCKER_REGISTRY)..."
	docker tag $(DOCKER_IMAGE):$(DOCKER_TAG) $(DOCKER_FULL_IMAGE)
	docker push $(DOCKER_FULL_IMAGE)

docker-clean: ## Remove Docker images
	@echo "Removing Docker images..."
	docker rmi $(DOCKER_IMAGE):$(DOCKER_TAG) || true
	docker rmi $(DOCKER_FULL_IMAGE) || true

docker-clean-cache: ## Clean local Docker BuildKit cache
	@echo "Cleaning local Docker BuildKit cache..."
	rm -rf /tmp/docker-cache
	docker builder prune -f

docker-build-fast: ## Build Docker image with optimized inline cache (no external deps)
	@echo "Building Docker image with optimized inline cache: $(DOCKER_IMAGE):$(DOCKER_TAG)"
	@echo "Attempting to pull existing image for cache..."
	@docker pull $(DOCKER_IMAGE):$(DOCKER_TAG) 2>/dev/null || echo "No existing image found"
	env DOCKER_BUILDKIT=1 docker build \
		--cache-from=$(DOCKER_IMAGE):$(DOCKER_TAG) \
		--build-arg BUILDKIT_INLINE_CACHE=1 \
		--build-arg PUTIO_SYNC_GIT_SHA=$(GIT_SHA) \
		-t $(DOCKER_IMAGE):$(DOCKER_TAG) .
	@echo "Image built successfully with inline cache: $(DOCKER_IMAGE):$(DOCKER_TAG)"

docker-cache-info: ## Show Docker cache information
	@echo "Docker BuildKit cache information:"
	@echo "Local cache: /tmp/docker-cache"
	@if [ -d /tmp/docker-cache ]; then \
		echo "Local cache size: $$(du -sh /tmp/docker-cache 2>/dev/null || echo 'N/A')"; \
	else \
		echo "Local cache: Not present"; \
	fi
	@echo "Registry cache: $(DOCKER_REGISTRY)/$(DOCKER_IMAGE):cache"
	@echo ""
	@echo "Available cache strategies (in order of ease of use):"
	@echo "  make docker-build-fast         - Optimized inline cache (works everywhere)"
	@echo "  make docker-build              - Basic BuildKit with inline cache"
	@echo "  make docker-build-local-cache  - Local filesystem cache (requires buildx setup)"
	@echo "  make docker-build-cache        - Registry-based cache (best for CI/CD)"

# ============================================================================
# Package Build Targets
# ============================================================================

PACKAGE_VERSION ?= 0.2.0
DEB_ARCH ?= amd64

build-deb: ## Build Debian package (use DEB_ARCH=amd64|arm64|armhf PACKAGE_VERSION=x.y.z)
	@echo "Building Debian package (version: $(PACKAGE_VERSION), arch: $(DEB_ARCH))..."
	@command -v dpkg-deb >/dev/null 2>&1 || { echo "dpkg-deb not installed. Install with: apt-get install dpkg-dev"; exit 1; }
	bash packaging/debian/build-deb.sh $(PACKAGE_VERSION) $(DEB_ARCH)
	@echo "Debian package built in build/ directory"

build-deb-all: ## Build Debian packages for all architectures
	@echo "Building Debian packages for all architectures..."
	$(MAKE) build-deb DEB_ARCH=amd64 PACKAGE_VERSION=$(PACKAGE_VERSION)
	$(MAKE) build-deb DEB_ARCH=arm64 PACKAGE_VERSION=$(PACKAGE_VERSION)
	$(MAKE) build-deb DEB_ARCH=armhf PACKAGE_VERSION=$(PACKAGE_VERSION)
	@echo "All Debian packages built!"

build-snap: ## Build Snap package
	@echo "Building Snap package..."
	@command -v snapcraft >/dev/null 2>&1 || { echo "snapcraft not installed. Install with: snap install snapcraft --classic"; exit 1; }
	cd snap && snapcraft --verbose
	@echo "Snap package built!"

build-snap-clean: ## Clean Snap build artifacts
	@echo "Cleaning Snap build artifacts..."
	cd snap && snapcraft clean

build-flatpak: ## Build Flatpak package
	@echo "Building Flatpak package..."
	@command -v flatpak-builder >/dev/null 2>&1 || { echo "flatpak-builder not installed"; exit 1; }
	@echo "Generating cargo sources..."
	bash packaging/flatpak/generate-cargo-sources.sh
	@echo "Building Flatpak..."
	flatpak-builder --force-clean --repo=repo build-flatpak com.putio.PutioSync.yaml
	flatpak build-bundle repo putio-sync.flatpak com.putio.PutioSync
	@echo "Flatpak bundle built: putio-sync.flatpak"

build-flatpak-clean: ## Clean Flatpak build artifacts
	@echo "Cleaning Flatpak build artifacts..."
	rm -rf build-flatpak repo putio-sync.flatpak cargo-sources.json

packages: ## Build all packages (Debian, Snap, Flatpak)
	@echo "Building all packages..."
	@echo "Note: This requires all package build tools to be installed"
	$(MAKE) build-deb || echo "Debian build failed or skipped"
	$(MAKE) build-snap || echo "Snap build failed or skipped"
	$(MAKE) build-flatpak || echo "Flatpak build failed or skipped"
	@echo "Package builds completed!"

# Release binaries and dhi.io images (feature 053). dhi.io needs `docker login dhi.io`.
DIST_ARCH ?= $(shell uname -m | sed 's/arm64/aarch64/')
DIST_PLATFORM = linux/$(if $(filter aarch64,$(DIST_ARCH)),arm64,amd64)
DHI_TARGET ?= runtime

dist-linux: ## Static musl Linux binary in dist/ (DIST_ARCH=x86_64|aarch64; other arch needs emulation)
	docker buildx build -f Dockerfile.dhi --target binary --platform $(DIST_PLATFORM) \
		--build-arg PUTIO_SYNC_GIT_SHA=$$(git rev-parse HEAD) \
		--output type=local,dest=dist/linux-$(DIST_ARCH) .
	file dist/linux-$(DIST_ARCH)/putio-sync

dist-macos: ## macOS .pkg installer in dist/ from a release build (PACKAGE_VERSION=x.y.z)
	cargo build --release --locked --package putio-sync-cli
	bash packaging/macos/build-pkg.sh $(PACKAGE_VERSION) target/release/putio-sync dist

docker-dhi: ## Build a dhi.io image (DHI_TARGET=runtime|runtime-ffmpeg|dev|dev-ffmpeg)
	docker buildx build -f Dockerfile.dhi --target $(DHI_TARGET) --load \
		--build-arg PUTIO_SYNC_GIT_SHA=$$(git rev-parse HEAD) \
		-t putio-sync:$(subst runtime,dhi,$(DHI_TARGET)) .

# ============================================================================
# GitHub Workflow Local Execution
# ============================================================================

workflow-ci: ## Run CI workflow locally using act (requires: brew install act)
	@echo "Running CI workflow locally with act..."
	@command -v act >/dev/null 2>&1 || { echo "act not installed. Install with: brew install act (macOS) or see https://github.com/nektos/act"; exit 1; }
	act -W .github/workflows/ci.yml

workflow-docker: ## Run Docker workflow locally using act
	@echo "Running Docker workflow locally with act..."
	@command -v act >/dev/null 2>&1 || { echo "act not installed. Install with: brew install act"; exit 1; }
	act -W .github/workflows/docker.yml

ci-docker: ## Run full CI checks in Docker container
	@echo "Running CI in Docker container..."
	docker run --rm -v $(PWD):/workspace -w /workspace rust:alpine sh -c "\
		apk add --no-cache musl-dev openssl-dev pkgconfig && \
		rustup component add rustfmt clippy && \
		cargo fmt --all -- --check && \
		cargo clippy --all-targets --all-features -- -D warnings && \
		cargo test --all-features --verbose && \
		cargo build --release --verbose"
	@echo "CI checks passed in Docker!"

# ============================================================================
# Combined Targets
# ============================================================================

all-builds: build docker-build ## Build binary and Docker image
	@echo "All builds completed!"

all-tests: test docker-test ## Run all tests (cargo + docker)
	@echo "All tests passed!"

clean-all: clean docker-clean build-snap-clean build-flatpak-clean ## Clean all build artifacts
	@echo "Cleaning all build artifacts..."
	rm -rf build/
	@echo "All artifacts cleaned!"
