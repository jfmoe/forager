#!/bin/bash
# PreToolUse hook for Bash: rejects a check command whose output is piped into tail, head or grep.
# The pipe replaces cargo's exit code with the filter's, so a failed check reads as a pass.
command=$(jq -r '.tool_input.command // empty')
unquoted=$(printf '%s' "$command" | sed -E "s/'[^']*'//g; s/\"[^\"]*\"//g; s/[0-9]*>&[0-9]+//g")
if printf '%s' "$unquoted" | grep -Eq 'cargo +(nextest +run|clippy)[^;&|]*\|&? *(tail|head|grep)([^[:alnum:]_-]|$)'; then
    echo "Run the check without a pipe: tail, head or grep replaces cargo's exit code, so a failure reads as a pass. For long output, redirect it to a file under .local/ and read the file." >&2
    exit 2
fi
exit 0
