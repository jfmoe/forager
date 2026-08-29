#!/usr/bin/env bash
set -euo pipefail

source_path=${1:?release target source is required}
output_path=${2:?GitHub output path is required}

targets=$(jq -ce '[.unix[], .windows[]] | select(length > 0)' "$source_path")
unix_matrix=$(jq -ce '{include: .unix} | select(.include | length > 0)' "$source_path")
windows_matrix=$(jq -ce '{include: .windows} | select(.include | length > 0)' "$source_path")

jq -en \
  --argjson targets "$targets" \
  --argjson unix_matrix "$unix_matrix" \
  --argjson windows_matrix "$windows_matrix" \
  '$targets == ($unix_matrix.include + $windows_matrix.include)
   and ([$targets[].target] | length == (unique | length))
   and all($unix_matrix.include[];
     (.target | type) == "string" and (.target | length) > 0
     and (.runner | type) == "string" and (.runner | length) > 0
     and (.host_arch | type) == "string" and (.host_arch | length) > 0
     and (.file_arch | type) == "string" and (.file_arch | length) > 0
     and .archive == "tar.xz")
   and all($windows_matrix.include[];
     (.target | type) == "string" and (.target | length) > 0
     and (.runner | type) == "string" and (.runner | length) > 0
     and (.machine | type) == "string" and (.machine | length) > 0
     and .archive == "zip")' \
  >/dev/null

{
  printf 'targets=%s\n' "$targets"
  printf 'unix-matrix=%s\n' "$unix_matrix"
  printf 'windows-matrix=%s\n' "$windows_matrix"
} >> "$output_path"
