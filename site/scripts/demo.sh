#!/bin/sh
# Starts a ramzes instance with demo data for the documentation's screenshots.
#   site/scripts/demo.sh            start it on http://127.0.0.1:4795 and fill it
#   site/scripts/demo.sh stop       stop it and remove its data
# Needs a debug build (`cargo build -p ramzes-server`), a built UI (`make ui`) and the sqlite3
# command, which spreads the demo events over the last two days so the charts have a shape.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
WORK="${TMPDIR:-/tmp}/ramzes-docs-demo"
APP=http://127.0.0.1:4795
RECEIVER=http://127.0.0.1:4796

stop() {
	for name in ramzes receiver; do
		[ -f "$WORK/$name.pid" ] && kill "$(cat "$WORK/$name.pid")" 2>/dev/null || true
	done
}

stop
if [ "${1:-}" = "stop" ]; then
	rm -rf "$WORK"
	exit 0
fi

rm -rf "$WORK"
mkdir -p "$WORK"
node "$REPO/ui/e2e/receiver.mjs" 4796 >/dev/null 2>&1 &
echo $! >"$WORK/receiver.pid"
RAMZES_LISTEN=127.0.0.1:4795 RAMZES_DATA_DIR="$WORK/data" "$REPO/target/debug/ramzes" >"$WORK/log" 2>&1 &
echo $! >"$WORK/ramzes.pid"
i=0
until curl -sf "$APP/healthz" >/dev/null; do
	i=$((i + 1))
	[ "$i" -lt 120 ] || { echo "ramzes did not start; see $WORK/log" >&2; exit 1; }
	sleep 0.5
done

node "$HERE/seed.mjs" "$APP" "$RECEIVER" "${EVENTS:-420}"
# Let the deliveries go out before the clock is turned back on them.
sleep 4
sqlite3 "$WORK/data/ramzes.db" "
CREATE TEMP TABLE shift AS SELECT id, (abs(random()) % 172800000) * (abs(random()) % 100 + 20) / 120 AS by FROM events;
UPDATE events SET received_at = received_at - (SELECT by FROM shift WHERE shift.id = events.id),
  occurred_at = occurred_at - (SELECT by FROM shift WHERE shift.id = events.id);
UPDATE deliveries SET created_at = created_at - COALESCE((SELECT by FROM shift JOIN notification_events ne
  ON ne.event_id = shift.id WHERE ne.notification_id = deliveries.notification_id LIMIT 1), 0);
"
echo "demo instance at $APP (kris@example.com / correct horse battery)"
