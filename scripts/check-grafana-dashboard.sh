#!/usr/bin/env bash
# The resources dashboard must be valid JSON and every query must name
# app="putio-sync": the main Prometheus only reads the long-term copy for
# queries that do (remote_read required_matchers).
#
# If HOME_NETWORK_DIR names a home-network checkout, the provisioned copy
# there must match this file, apart from data source fields and the Jinja raw
# markers. The compare is skipped when the variable is unset or the dir is missing.
set -euo pipefail

cd "$(dirname "$0")/.."
dashboard=packaging/grafana/putio-sync-resources.json
copy=roles/monitoring_grafana/templates/putio-sync-resources-dashboard.json.j2

jq empty "$dashboard"

missing=$(jq -r '
    [(.panels[] | .panels[]?, .), .annotations.list[]?]
    | .[] | (.targets[]?, .) | .expr? // empty
    | select(contains("app=\"putio-sync\"") | not)' "$dashboard")
if [ -n "$missing" ]; then
    echo "$dashboard: queries without app=\"putio-sync\":"
    echo "$missing"
    exit 1
fi

if [ -z "${HOME_NETWORK_DIR:-}" ] || [ ! -d "$HOME_NETWORK_DIR" ]; then
    exit 0
fi

normalize='del(.__inputs, .__requires) | walk(if type == "object" then del(.datasource) else . end)'
if ! diff -u \
    <(jq -S "$normalize" "$dashboard") \
    <(grep -v -e '^{% raw %}$' -e '^{% endraw %}$' "$HOME_NETWORK_DIR/$copy" | jq -S "$normalize") \
    --label "$dashboard" --label "$HOME_NETWORK_DIR/$copy"; then
    echo "The home-network copy differs from $dashboard"
    exit 1
fi
echo "Dashboard matches $HOME_NETWORK_DIR/$copy"
