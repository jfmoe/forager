#!/usr/bin/env bash
set -euo pipefail

artifacts_dir=${1:?artifacts directory is required}

cd "$artifacts_dir"
checksum_count=$(find . -type f -name '*.sha256' | wc -l | tr -d ' ')
test "$checksum_count" -eq "$(jq 'length' <<< "$TARGETS")"
find . -type f -name '*.sha256' -print0 |
  while IFS= read -r -d '' checksum; do
    sha256sum --check "$checksum"
  done
