#!/usr/bin/env bash
# Keeps one long-lived issue for a scheduled workflow, found by its label: a failed run opens it
# or comments on it, and a successful run closes it. RESULTS holds the result of every job the
# report waits for; the run succeeded only when each is `success`.
set -euo pipefail

label=${1:?the issue label is required}
title=${2:?the issue title is required}
run_url="$GITHUB_SERVER_URL/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID"

succeeded=true
for result in ${RESULTS:?the job results are required}; do
  [[ $result == success ]] || succeeded=false
done

number=$(gh issue list --repo "$GITHUB_REPOSITORY" --label "$label" --state open --limit 1 \
  --json number --jq '.[0].number // ""')

if [[ $succeeded == true ]]; then
  if [[ -n $number ]]; then
    gh issue close "$number" --repo "$GITHUB_REPOSITORY" --comment "Passed again in $run_url."
  fi
elif [[ -n $number ]]; then
  gh issue comment "$number" --repo "$GITHUB_REPOSITORY" --body "Failed again in $run_url."
else
  gh label create "$label" --repo "$GITHUB_REPOSITORY" --force --color D93F0B \
    --description "A scheduled workflow's long-lived failure issue"
  gh issue create --repo "$GITHUB_REPOSITORY" --label "$label" --title "$title" \
    --body "The scheduled workflow failed in $run_url. A later successful run closes this issue."
fi
