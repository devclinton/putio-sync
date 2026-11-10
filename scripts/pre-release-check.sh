#!/bin/bash
set -e

# Pre-release validation script
# Ensures everything is ready before creating a release

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

ERRORS=0
WARNINGS=0

error() {
    echo -e "${RED}✗ ERROR:${NC} $1"
    ERRORS=$((ERRORS + 1))
}

warning() {
    echo -e "${YELLOW}⚠ WARNING:${NC} $1"
    WARNINGS=$((WARNINGS + 1))
}

success() {
    echo -e "${GREEN}✓${NC} $1"
}

info() {
    echo -e "${BLUE}ℹ${NC} $1"
}

section() {
    echo ""
    echo -e "${BLUE}━━━ $1 ━━━${NC}"
}

# Get version from argument or Cargo.toml
if [ -n "$1" ]; then
    VERSION="$1"
else
    VERSION=$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
fi

info "Checking release readiness for version ${VERSION}"
echo ""

# Check 1: Version consistency
section "Version Consistency"

check_version_in_file() {
    FILE=$1
    PATTERN=$2

    if grep -q "${PATTERN}${VERSION}" "$FILE"; then
        success "Version ${VERSION} found in $FILE"
    else
        error "Version ${VERSION} not found in $FILE"
        info "  Current version: $(grep "$PATTERN" "$FILE" | head -1)"
    fi
}

check_version_in_file "Cargo.toml" '^version = "'
check_version_in_file "snap/snapcraft.yaml" "^version: '"
check_version_in_file "packaging/debian/DEBIAN/control" "^Version: "

# Check Homebrew formula
if grep -q "version \"${VERSION}\"" packaging/homebrew/putio-sync.rb; then
    success "Version ${VERSION} found in Homebrew formula"
else
    warning "Version in Homebrew formula will be auto-updated"
fi

# Check Winget manifest
if grep -q "PackageVersion: ${VERSION}" packaging/winget/manifests/devclinton.putio-sync.installer.yaml; then
    success "Version ${VERSION} found in Winget installer manifest"
else
    warning "Winget installer manifest is not at ${VERSION} (the release workflow writes its own)"
    info "  Update PackageVersion in packaging/winget/manifests/devclinton.putio-sync.installer.yaml before a manual winget-pkgs submission"
fi

if grep -q "PackageVersion: ${VERSION}" packaging/winget/manifests/devclinton.putio-sync.locale.en-US.yaml; then
    success "Version ${VERSION} found in Winget locale manifest"
else
    warning "Winget locale manifest is not at ${VERSION} (the release workflow writes its own)"
    info "  Update PackageVersion in packaging/winget/manifests/devclinton.putio-sync.locale.en-US.yaml before a manual winget-pkgs submission"
fi

if grep -q "PackageVersion: ${VERSION}" packaging/winget/manifests/devclinton.putio-sync.yaml; then
    success "Version ${VERSION} found in Winget version manifest"
else
    warning "Winget version manifest is not at ${VERSION} (the release workflow writes its own)"
    info "  Update PackageVersion in packaging/winget/manifests/devclinton.putio-sync.yaml before a manual winget-pkgs submission"
fi

# Check 2: Changelog
section "Changelog"

if grep -q "## \[${VERSION}\]" CHANGELOG.md; then
    success "Changelog entry for ${VERSION} exists"

    # Check for "Unreleased" entries
    if grep -q "## \[Unreleased\]" CHANGELOG.md; then
        if [ "$(sed -n '/## \[Unreleased\]/,/## \[/p' CHANGELOG.md | wc -l)" -gt 3 ]; then
            warning "Unreleased section has content - consider moving to ${VERSION}"
        fi
    fi
else
    error "No changelog entry for ${VERSION} found"
    info "  Add an entry in CHANGELOG.md"
fi

# Check 3: Git status
section "Git Status"

if [ -n "$(git status --porcelain)" ]; then
    warning "Working directory has uncommitted changes"
    git status --short
else
    success "Working directory is clean"
fi

# Check if on main branch
BRANCH=$(git branch --show-current)
if [ "$BRANCH" != "main" ]; then
    warning "Not on main branch (currently on: $BRANCH)"
else
    success "On main branch"
fi

# Check 4: Tests
section "Tests"

info "Running test suite..."
if cargo test --workspace --quiet 2>&1 | tail -5; then
    success "All tests pass"
else
    error "Tests failed"
fi

# Check 5: Build
section "Build"

info "Running release build..."
if cargo build --release --quiet 2>&1 | tail -5; then
    success "Release build successful"
else
    error "Release build failed"
fi

# Check 6: Security Audit
section "Security Audit"

if command -v cargo-audit &> /dev/null; then
    info "Running security audit..."
    if cargo audit --quiet; then
        success "No security vulnerabilities found"
    else
        warning "Security audit found issues"
    fi
else
    warning "cargo-audit not installed (install with: cargo install cargo-audit)"
fi

# Check 7: Formatting
section "Code Formatting"

if cargo fmt -- --check; then
    success "Code is formatted correctly"
else
    warning "Code formatting issues found (run: cargo fmt)"
fi

# Check 8: Clippy
section "Linting (Clippy)"

if cargo clippy --workspace --quiet -- -D warnings 2>&1 | tail -10; then
    success "No clippy warnings"
else
    warning "Clippy found issues"
fi

# Check 9: Documentation
section "Documentation"

info "Checking documentation..."
if cargo doc --no-deps --quiet 2>&1 | tail -5; then
    success "Documentation builds successfully"
else
    warning "Documentation has issues"
fi

# Check 10: Package files exist
section "Packaging Files"

check_file() {
    if [ -f "$1" ]; then
        success "$1 exists"
    else
        error "$1 missing"
    fi
}

check_file "packaging/homebrew/putio-sync.rb"
check_file "packaging/winget/manifests/devclinton.putio-sync.yaml"
check_file "packaging/debian/DEBIAN/control"
check_file "snap/snapcraft.yaml"
check_file "io.github.devclinton.putio-sync.yaml"

# Summary
section "Summary"

if [ $ERRORS -eq 0 ] && [ $WARNINGS -eq 0 ]; then
    echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${GREEN}✓ All checks passed! Ready to release.${NC}"
    echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""
    info "To create release:"
    echo "  git tag -a v${VERSION} -m \"Release v${VERSION}\""
    echo "  git push origin v${VERSION}"
    exit 0
elif [ $ERRORS -eq 0 ]; then
    echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${YELLOW}⚠ ${WARNINGS} warning(s) found${NC}"
    echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""
    info "You can proceed, but please review the warnings"
    exit 0
else
    echo -e "${RED}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${RED}✗ ${ERRORS} error(s) and ${WARNINGS} warning(s) found${NC}"
    echo -e "${RED}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""
    error "Please fix the errors before releasing"
    exit 1
fi
