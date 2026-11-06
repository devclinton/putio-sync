#!/bin/bash
# Daily archive of putio-sync resource telemetry on Unraid (feature 041).
#
# Install with the "User Scripts" plugin: add a script, paste this file, set the schedule
# to "Custom" with `30 3 * * *`. Change P to your monitoring server's Prometheus address.
# The archive lands in /config/archive inside the container (appdata/putio-sync/archive).
set -euo pipefail
P=http://192.168.1.60:9090
run() { docker exec putio-sync putio-sync --config /config/config.toml archive "$@"; }
run metrics --prometheus "$P" --require-manifest
run summary --prometheus "$P"
# weekly, Mondays, before the backup restart window; an `if` keeps the exit status 0 on
# the other days (a trailing `[ ... ] && run db` would end the script with status 1)
if [ "$(date +%u)" = 1 ]; then run db; fi
