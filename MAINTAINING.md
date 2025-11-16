# Maintainer Guide

This guide is for maintainers of the putio-sync project, covering release procedures, package management, and ongoing maintenance tasks.

## Table of Contents

- [Release Process](#release-process)
- [Package Management](#package-management)
- [GitHub Secrets](#github-secrets)
- [Monitoring](#monitoring)
- [Troubleshooting CI/CD](#troubleshooting-cicd)
- [Security](#security)
- [Community Management](#community-management)

---

## Release Process

### Pre-Release Checklist

1. **Update Version Numbers**

   Edit all version files:
   ```bash
   # Cargo.toml (workspace)
   vim Cargo.toml

   # Update snap version
   vim snap/snapcraft.yaml

   # Update Debian control
   vim packaging/debian/DEBIAN/control
   ```

2. **Update Changelog**

   Add release notes to `putio-sync-cli/CHANGELOG.md`:
   ```markdown
   ## [0.1.2] - 2025-11-16

   ### Added
   - New feature description

   ### Fixed
   - Bug fix description
   ```

3. **Run Pre-Release Validation**

   ```bash
   bash scripts/pre-release-check.sh 0.1.2
   ```

   This checks:
   - Version consistency
   - Changelog updates
   - Test suite
   - Code formatting
   - Security audit
   - Documentation

4. **Commit Version Bump**

   ```bash
   git add .
   git commit -m "chore: bump version to 0.1.2"
   git push origin main
   ```

### Creating a Release

1. **Tag the Release**

   ```bash
   git tag -a v0.1.2 -m "Release v0.1.2"
   git push origin v0.1.2
   ```

2. **Monitor Workflows**

   The release workflow will automatically:
   - Create GitHub release
   - Build Windows binaries (x64, ARM64)
   - Build Debian packages (amd64, arm64)
   - Build and publish Snap
   - Build Flatpak bundle
   - Update Homebrew formula
   - Submit to Winget

   Monitor at: https://github.com/devclinton/putio-sync/actions

3. **Verify Release Assets**

   Check that all artifacts uploaded:
   - Windows ZIPs (x64, ARM64)
   - Debian .deb files (amd64, arm64)
   - Flatpak bundle
   - SHA256 checksums

### Post-Release Tasks

1. **Verify Package Availability**

   Test installation on each platform:
   ```bash
   # macOS
   brew upgrade putio-sync

   # Windows
   winget upgrade putio.putio-sync

   # Linux (Snap)
   sudo snap refresh putio-sync

   # Linux (Flatpak)
   flatpak update com.putio.PutioSync
   ```

2. **Monitor Package Submissions**

   - **Winget**: Check PR at https://github.com/microsoft/winget-pkgs/pulls
   - **Snap**: Verify at https://snapcraft.io/putio-sync
   - **Homebrew**: Check tap at https://github.com/devclinton/homebrew-putio-sync

3. **Announce Release**

   - Update GitHub release notes if needed
   - Post to discussions (if enabled)
   - Tweet/social media (optional)

---

## Package Management

### Homebrew

**Repository**: https://github.com/devclinton/homebrew-putio-sync

**Automated Updates**: Yes (via `.github/workflows/homebrew.yml`)

**Manual Update** (if needed):
```bash
# Clone tap repository
git clone https://github.com/devclinton/homebrew-putio-sync.git
cd homebrew-putio-sync

# Update Formula/putio-sync.rb
vim Formula/putio-sync.rb

# Update version, URL, and SHA256
git add Formula/putio-sync.rb
git commit -m "Update putio-sync to v0.1.2"
git push
```

### Winget

**Automated Submission**: Yes (via `.github/workflows/winget.yml`)

**Monitor PRs**: https://github.com/microsoft/winget-pkgs/pulls

**Manual Submission** (if needed):
1. Fork https://github.com/microsoft/winget-pkgs
2. Update manifests in `manifests/p/putio/putio-sync/VERSION/`
3. Create pull request
4. Address validation feedback

### Snap

**Store**: https://snapcraft.io/putio-sync

**Automated Publishing**: Yes (via `.github/workflows/snap.yml`)

**Manual Publishing** (if needed):
```bash
snapcraft login
snapcraft upload putio-sync_VERSION.snap --release stable
```

**Channel Management**:
```bash
# List releases
snapcraft releases putio-sync

# Promote from edge to stable
snapcraft promote putio-sync --from-channel edge --to-channel stable
```

### Flatpak

**Flathub Repository**: https://github.com/flathub/com.putio.PutioSync

**Automated Builds**: Bundles via workflow, manual Flathub submission

**Update Flathub**:
1. Update manifest in Flathub repository
2. Create pull request
3. Address review feedback
4. Flathub builds and publishes automatically

---

## GitHub Secrets

Required secrets for automated workflows:

### HOMEBREW_TAP_TOKEN

**Purpose**: Update Homebrew tap repository

**Permissions**: `repo`, `workflow`

**Setup**:
1. Go to https://github.com/settings/tokens/new
2. Create token with `repo` and `workflow` scopes
3. Add to repository secrets: https://github.com/devclinton/putio-sync/settings/secrets/actions

### SNAPCRAFT_TOKEN

**Purpose**: Publish to Snap Store

**Setup**:
```bash
snapcraft login
snapcraft export-login --snaps=putio-sync --channels=stable,candidate,beta,edge - > snapcraft-token.txt
# Add contents to GitHub secret
```

### GITHUB_TOKEN

**Purpose**: Upload release assets

**Setup**: Automatically provided by GitHub Actions (no manual setup)

---

## Monitoring

### Download Metrics

- **GitHub Releases**: https://github.com/devclinton/putio-sync/releases
- **Snap Store**: https://snapcraft.io/putio-sync/metrics
- **Homebrew**: `brew info putio-sync` shows install count

### Build Status

Monitor workflow runs:
```bash
gh run list --repo devclinton/putio-sync
```

### Issue Tracking

- **Open Issues**: https://github.com/devclinton/putio-sync/issues
- **Pull Requests**: https://github.com/devclinton/putio-sync/pulls

Set up notifications for:
- New issues
- Failed workflows
- Security advisories

---

## Troubleshooting CI/CD

### Release Workflow Fails

**Check**:
1. Workflow logs: https://github.com/devclinton/putio-sync/actions
2. Individual platform workflows
3. Release creation step

**Common Issues**:
- Missing tag
- Invalid version format
- Workflow permissions

### Homebrew Update Fails

**Check**:
- HOMEBREW_TAP_TOKEN is valid
- Tap repository exists
- Formula syntax is correct

**Debug**:
```bash
brew install --build-from-source ./packaging/homebrew/putio-sync.rb
```

### Winget Submission Fails

**Common Issues**:
- Windows binaries not found
- SHA256 mismatch
- Manifest validation errors

**Check PR**:
- Look at automated validation comments
- Fix manifest files locally
- Re-run workflow

### Snap Publishing Fails

**Check**:
- SNAPCRAFT_TOKEN is valid and not expired
- Snap name is registered
- Build succeeded

**Manual publish**:
```bash
snapcraft upload --release=stable putio-sync_VERSION.snap
```

### Debian Build Fails

**Common Issues**:
- Missing dependencies
- Cross-compilation setup
- File permissions

**Test locally**:
```bash
bash packaging/debian/build-deb.sh 0.1.2 amd64
```

---

## Security

### Dependency Updates

**Regular audits**:
```bash
cargo audit
cargo outdated
```

**Update dependencies**:
```bash
cargo update
cargo test
```

### Security Advisories

Monitor:
- https://github.com/devclinton/putio-sync/security/advisories
- Dependabot alerts

**Response Process**:
1. Assess severity
2. Update affected dependencies
3. Test thoroughly
4. Release patch version
5. Notify users if critical

### Code Signing

**Future Enhancement**: Consider code signing for:
- macOS binaries (Apple Developer ID)
- Windows binaries (Authenticode)

Benefits:
- Reduces SmartScreen warnings
- Increases user trust
- Required for some distribution channels

---

## Community Management

### Issue Triage

**Priority Labels**:
- `critical`: Security issues, data loss
- `high`: Major features broken
- `medium`: Minor bugs, enhancement requests
- `low`: Documentation, nice-to-have features

**Response Time Goals**:
- Critical: Within 24 hours
- High: Within 3 days
- Medium: Within 1 week
- Low: Best effort

### Pull Request Review

**Checklist**:
- [ ] Tests pass
- [ ] Code follows style guidelines
- [ ] Documentation updated
- [ ] Changelog entry added (if user-facing)
- [ ] No security issues
- [ ] Commit messages are clear

### Communication

**Channels**:
- GitHub Issues: Bug reports, feature requests
- GitHub Discussions: General questions, ideas
- Pull Requests: Code contributions

**Response Templates**:

See `.github/ISSUE_TEMPLATE/` for:
- Bug report template
- Feature request template
- Pull request template

---

## Versioning

Follow [Semantic Versioning](https://semver.org/):

- **MAJOR**: Breaking changes
- **MINOR**: New features (backward compatible)
- **PATCH**: Bug fixes (backward compatible)

Examples:
- `0.1.0` → `0.2.0`: New command added
- `0.1.0` → `0.1.1`: Bug fix in download
- `0.1.0` → `1.0.0`: Breaking CLI changes

---

## Rollback Procedure

If a release has critical issues:

1. **Delete Git Tag**
   ```bash
   git tag -d v0.1.2
   git push origin :refs/tags/v0.1.2
   ```

2. **Delete GitHub Release**
   - Go to releases page
   - Delete the release

3. **Revert Package Submissions**
   - Winget: Close PR if not merged
   - Snap: `snapcraft close putio-sync edge`
   - Homebrew: Revert commit in tap

4. **Fix and Re-Release**
   ```bash
   # Fix the issue
   git commit -m "fix: critical issue"

   # Create patch version
   git tag -a v0.1.3 -m "Release v0.1.3"
   git push origin v0.1.3
   ```

---

## Resources

- [Release Script](scripts/pre-release-check.sh)
- [Homebrew Documentation](packaging/homebrew/README.md)
- [Winget Documentation](packaging/winget/README.md)
- [Snap Documentation](snap/README.md)
- [Flatpak Documentation](packaging/flatpak/README.md)
- [Debian Documentation](packaging/debian/README.md)

---

## Questions?

For maintainer-specific questions:
- Open a discussion: https://github.com/devclinton/putio-sync/discussions
- Contact: devclinton@users.noreply.github.com
