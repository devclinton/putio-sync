# Release Scripts

This directory contains scripts for managing releases and validating the project before publishing.

## Pre-Release Check

**Script**: `pre-release-check.sh`

Validates that everything is ready before creating a release.

### Usage

```bash
# Check current version from Cargo.toml
bash scripts/pre-release-check.sh

# Check specific version
bash scripts/pre-release-check.sh 0.1.2
```

### Checks Performed

1. **Version Consistency**: Ensures version matches across all package files
   - Cargo.toml
   - snap/snapcraft.yaml
   - packaging/debian/DEBIAN/control
   - Homebrew formula (warning only)

2. **Changelog**: Verifies changelog entry exists for the version

3. **Git Status**: Checks for uncommitted changes and branch

4. **Tests**: Runs full test suite

5. **Build**: Performs release build

6. **Security Audit**: Runs `cargo audit` if available

7. **Formatting**: Verifies code formatting with `cargo fmt`

8. **Linting**: Checks for clippy warnings

9. **Documentation**: Builds documentation

10. **Packaging Files**: Verifies all packaging files exist

### Exit Codes

- **0**: All checks passed or only warnings
- **1**: Errors found, release not recommended

### Example Output

```
ℹ Checking release readiness for version 0.1.2

━━━ Version Consistency ━━━
✓ Version 0.1.2 found in Cargo.toml
✓ Version 0.1.2 found in snap/snapcraft.yaml
✓ Version 0.1.2 found in packaging/debian/DEBIAN/control

━━━ Changelog ━━━
✓ Changelog entry for 0.1.2 exists

━━━ Git Status ━━━
✓ Working directory is clean
✓ On main branch

━━━ Tests ━━━
ℹ Running test suite...
✓ All tests pass

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
✓ All checks passed! Ready to release.
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

ℹ To create release:
  git tag -a v0.1.2 -m "Release v0.1.2"
  git push origin v0.1.2
```

## Release Process

### 1. Prepare Release

```bash
# Update version in Cargo.toml
vim Cargo.toml

# Update CHANGELOG.md
vim putio-sync-cli/CHANGELOG.md

# Update package versions
vim snap/snapcraft.yaml
vim packaging/debian/DEBIAN/control

# Commit changes
git add .
git commit -m "chore: bump version to 0.1.2"
```

### 2. Run Pre-Release Check

```bash
bash scripts/pre-release-check.sh 0.1.2
```

### 3. Create Release

If all checks pass:

```bash
# Create and push tag
git tag -a v0.1.2 -m "Release v0.1.2"
git push origin v0.1.2
```

### 4. Monitor Workflows

The release workflow will automatically:
1. Create GitHub release
2. Build Windows binaries
3. Build Debian packages
4. Build and publish Snap
5. Build Flatpak bundle
6. Update Homebrew formula
7. Submit to Winget

Check progress at:
```
https://github.com/clintoncollins/putio-sync/actions
```

### 5. Post-Release

After workflows complete:
1. Verify all packages are available
2. Test installation on each platform
3. Update documentation if needed
4. Announce release

## Troubleshooting

### Version Mismatch Errors

Update all package files to match:
```bash
# Find version mismatches
grep -r "version.*0.1.1" packaging/ snap/

# Update manually or use sed
sed -i 's/0.1.1/0.1.2/g' snap/snapcraft.yaml
```

### Test Failures

Run tests with verbose output:
```bash
cargo test --workspace -- --nocapture
```

### Build Failures

Check build with full output:
```bash
cargo build --release --verbose
```

### Clippy Warnings

Fix automatically where possible:
```bash
cargo clippy --workspace --fix
```

### Format Issues

Auto-format code:
```bash
cargo fmt
```

## CI Integration

The pre-release check can be integrated into CI:

```yaml
# .github/workflows/pre-release.yml
on:
  pull_request:
    branches: [main]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: bash scripts/pre-release-check.sh
```

## Adding New Checks

To add a new validation check:

1. Add a new section to `pre-release-check.sh`
2. Use the helper functions: `error()`, `warning()`, `success()`, `info()`
3. Increment error/warning counters as appropriate
4. Document the check in this README

Example:
```bash
section "My New Check"

if my_validation_command; then
    success "Validation passed"
else
    error "Validation failed"
fi
```
