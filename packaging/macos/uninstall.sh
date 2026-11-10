#!/bin/sh
# Remove what the putio-sync installer added. Your config (~/.putiosync),
# logs and downloads stay. Run with sudo:
#   sudo /usr/local/share/putio-sync/uninstall.sh
set -e
plist=/Library/LaunchAgents/com.devclinton.putio-sync.plist
user="$(stat -f %Su /dev/console)"
if [ -n "$user" ] && [ "$user" != "root" ]; then
    launchctl bootout "gui/$(id -u "$user")/com.devclinton.putio-sync" 2>/dev/null || true
fi
rm -f "$plist" /usr/local/bin/putio-sync "/Applications/Put.io Sync Web UI.webloc"
rm -rf /usr/local/share/putio-sync
pkgutil --forget com.devclinton.putio-sync >/dev/null 2>&1 || true
pkgutil --forget com.devclinton.putio-sync.agent >/dev/null 2>&1 || true
echo "putio-sync removed"
