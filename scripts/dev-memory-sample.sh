#!/usr/bin/env bash
# Samples the memory footprint (what Activity Monitor's "Memory" column reports) of a running
# Procyon desktop app and the WebKit helper processes it started, once every INTERVAL seconds.
#
#   scripts/dev-memory-sample.sh [app-process-pattern] [interval-seconds]
#
# macOS only. Helpers are the WebContent/GPU/Networking XPC services started within 20 s of the
# app; windows or panels opened later are not counted. Pair the output with the app's own cache statistics in
# ~/Library/Application Support/procyon/procyon-desktop.log.* (`stat-cache` entries) by timestamp.
set -euo pipefail

pattern="${1:-fm-desktop}"
interval="${2:-15}"
app_pid="$(pgrep -f "$pattern" | head -1 || true)"
if [[ -z "$app_pid" ]]; then
  echo "no running process matches '$pattern'" >&2
  exit 1
fi

# "Footprint: 94 MB" / "1.2 GB" / "512 KB" -> megabytes
footprint_mb() {
  footprint "$1" 2>/dev/null | sed -n 's/.*Footprint: \([0-9.]*\) \([KMG]B\).*/\1 \2/p' | head -1 |
    awk '{
      if ($2 == "GB") printf "%.0f", $1 * 1024;
      else if ($2 == "KB") printf "%.0f", $1 / 1024;
      else printf "%.0f", $1 }'
}

# WebKit starts the app's WebContent/GPU/Networking services right after the app itself, so match
# them by age rather than pid, which wraps and is shared with other apps. macOS ps only has the
# formatted `etime` ([[dd-]hh:]mm:ss), converted to seconds here.
AWK_SECONDS='function secs(t,   d, parts, n, i, total) {
  d = 0; if (index(t, "-")) { split(t, a, "-"); d = a[1]; t = a[2] }
  n = split(t, parts, ":"); total = 0
  for (i = 1; i <= n; i++) total = total * 60 + parts[i]
  return d * 86400 + total }'
helper_pids() {
  local kind="$1" app_age
  app_age="$(ps -o etime= -p "$app_pid" | awk "$AWK_SECONDS"' { print secs($1) }')"
  ps -axo pid=,etime=,command= | awk -v kind="$kind" -v app_age="$app_age" "$AWK_SECONDS"'
    index($0, "com.apple.WebKit." kind ".xpc") { age = secs($2)
      if (age <= app_age && age >= app_age - 20) print $1 }'
}

echo "time,app_mb,webcontent_mb,gpu_mb,networking_mb,total_mb  (app pid $app_pid)"
while kill -0 "$app_pid" 2>/dev/null; do
  app="$(footprint_mb "$app_pid")"
  web=0; gpu=0; net=0
  for pid in $(helper_pids WebContent); do web=$((web + $(footprint_mb "$pid"))); done
  for pid in $(helper_pids GPU); do gpu=$((gpu + $(footprint_mb "$pid"))); done
  for pid in $(helper_pids Networking); do net=$((net + $(footprint_mb "$pid"))); done
  echo "$(date +%H:%M:%S),${app:-0},$web,$gpu,$net,$((${app:-0} + web + gpu + net))"
  sleep "$interval"
done
