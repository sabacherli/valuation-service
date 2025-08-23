#!/usr/bin/env bash
set -euo pipefail

# Location-independent starter for PostgreSQL + backend (valuation-service)
# Can be run from any directory. Intended to live inside the valuation-service repo.

# Resolve script and repo paths
SCRIPT_DIR="$(cd -- "$(dirname -- "$0")" >/dev/null 2>&1; pwd -P)"
BACKEND_DIR="$SCRIPT_DIR"

export DATABASE_URL="${DATABASE_URL:-postgres://postgres:postgres@localhost:5433/valuation}"
echo "Using DATABASE_URL=$DATABASE_URL"

# Ensure Postgres is running (ignore if already started)
if command -v service >/dev/null 2>&1; then
  sudo service postgresql start || true
fi

# Start backend in background
pushd "$BACKEND_DIR" >/dev/null
cargo run --release &
BACK_PID=$!
popd >/dev/null

echo "Backend started (pid=$BACK_PID)."

# Give backend a moment to bind port 3000
sleep 1

# Kill backend on exit
cleanup() {
  echo "\nStopping backend (pid=$BACK_PID)..."
  kill "$BACK_PID" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# Wait on backend (foreground)
wait "$BACK_PID"
