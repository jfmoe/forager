#!/usr/bin/env bash
set -euo pipefail

version=$(jq -r '.releases[0].app_version' <<< "$PLAN")
install_dir="$RUNNER_TEMP/forager-release"
config_dir="$RUNNER_TEMP/forager-config"
state_dir="$RUNNER_TEMP/forager-state"
home_dir="$RUNNER_TEMP/forager-home"
mkdir -p "$install_dir" "$state_dir" "$home_dir"
install -d -m 700 "$config_dir"
tar -xJf "artifacts/forager-$TARGET.$ARCHIVE" -C "$install_dir"
binary=$(find "$install_dir" -type f -name forager -print -quit)
test -n "$binary"
test -x "$binary"
test "$(uname -m)" = "$HOST_ARCH"
file "$binary" | grep -F "$FILE_ARCH"
binary_dir=$(dirname "$binary")
PATH="$binary_dir:$PATH"
export PATH
test "$(command -v forager)" = "$binary"
test "$(forager --version)" = "forager $version"
printf '%s\n' \
  '[providers.xai]' \
  'keys = ["release-gate"]' \
  '[providers.openai_compatible]' \
  'url = "https://example.invalid/v1"' \
  'keys = ["release-gate"]' \
  'model = "release-gate"' \
  '[providers.tavily]' \
  'keys = ["release-gate"]' \
  '[providers.firecrawl]' \
  'keys = ["release-gate"]' \
  '[providers.jina]' \
  'keys = ["release-gate"]' \
  '[providers.context7]' \
  'keys = ["release-gate"]' \
  '[providers.exa]' \
  'keys = ["release-gate"]' \
  '[providers.anysearch]' \
  'keys = ["release-gate"]' \
  > "$config_dir/config.toml"
chmod 600 "$config_dir/config.toml"
doctor=$(FORAGER_CONFIG_DIR="$config_dir" XDG_STATE_HOME="$state_dir" \
  HOME="$home_dir" forager doctor --timeout 1 || test "$?" -eq 4)
jq -e '
  .ok == false
  and .mode == "shallow"
  and ([.providers[] | select(.configured)] | length) == 8
  and .config.providers.exa.keys.source == "file"
  and (.permission_warnings | length) == 0
' <<< "$doctor"
