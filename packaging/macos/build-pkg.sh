#!/usr/bin/env bash
# Build the macOS installer (feature 053):
#   packaging/macos/build-pkg.sh <version> <putio-sync binary> [out dir]
# Writes putio-sync-<version>-macos-<arch>.pkg. The installer puts the binary
# in /usr/local/bin and a "Put.io Sync Web UI" link in /Applications, and
# offers a launchd agent (on by default) that runs the daemon at login.
#
# Set MACOS_INSTALLER_IDENTITY to a "Developer ID Installer: ..." identity in
# the keychain to sign it; without one the package is unsigned.
set -euo pipefail

version="${1:?version}"
binary="${2:?path to the putio-sync binary}"
out="${3:-dist}"
version="${version#v}"
here="$(cd "$(dirname "$0")" && pwd)"
arch="$(lipo -archs "$binary" 2>/dev/null || uname -m)"
arch="${arch// /-}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Component 1: the binary, the web UI link and the uninstaller
core="$work/core"
install -d "$core/usr/local/bin" "$core/usr/local/share/putio-sync" "$core/Applications"
install -m 0755 "$binary" "$core/usr/local/bin/putio-sync"
install -m 0755 "$here/uninstall.sh" "$core/usr/local/share/putio-sync/uninstall.sh"
cat > "$core/Applications/Put.io Sync Web UI.webloc" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>URL</key>
	<string>http://localhost:15432/</string>
</dict>
</plist>
EOF

# Component 2: the launchd agent
agent="$work/agent"
install -d "$agent/Library/LaunchAgents"
install -m 0644 "$here/com.devclinton.putio-sync.plist" "$agent/Library/LaunchAgents/"

# Extended attributes would land in the payload as ._ files. Quarantine clears;
# com.apple.provenance, set on files a sandboxed app writes, can't be.
xattr -cr "$core" "$agent"

pkgbuild --root "$core" --identifier com.devclinton.putio-sync --version "$version" \
    --install-location / "$work/core.pkg"
pkgbuild --root "$agent" --identifier com.devclinton.putio-sync.agent --version "$version" \
    --scripts "$here/scripts" --install-location / "$work/agent.pkg"

sed "s/@VERSION@/$version/g" "$here/distribution.xml" > "$work/distribution.xml"
mkdir "$work/resources"
cp "$here/resources/welcome.html" "$work/resources/"
cp "$here/../../LICENSE" "$work/resources/LICENSE.txt"
mkdir -p "$out"
pkg="$out/putio-sync-$version-macos-$arch.pkg"
sign=()
if [ -n "${MACOS_INSTALLER_IDENTITY:-}" ]; then
    sign=(--sign "$MACOS_INSTALLER_IDENTITY")
fi
productbuild --distribution "$work/distribution.xml" --package-path "$work" \
    --resources "$work/resources" ${sign[@]+"${sign[@]}"} "$pkg"
echo "$pkg"
