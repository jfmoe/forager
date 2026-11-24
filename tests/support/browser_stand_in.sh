#!/bin/sh
# The browser stand-in of the command contract tests. Each stand-in is a link to this script in a
# directory of its own, which holds the arguments of every launch and the `body` the launch runs.
dir=${0%/*}
for argument in "$@"; do printf '%s\n' "$argument" >> "$dir/argv"; done
printf '%s\n' '<<end of call>>' >> "$dir/argv"
. "$dir/body"
