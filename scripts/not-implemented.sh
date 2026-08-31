#!/usr/bin/env bash
# Prints a clear failure instead of silently no-op'ing for root scripts whose
# feature is not implemented yet.
#
# Usage: not-implemented.sh <script-name> <task-number>
set -euo pipefail

script_name="${1:?script name required}"
task_number="${2:?task number required}"

echo "error: '${script_name}' is not implemented until task ${task_number}; see TASKS/${task_number}-*.md" >&2
exit 1
