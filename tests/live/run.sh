#!/usr/bin/env bash
# Starts a local 3-node Perch network seeded with the demo dataset.
#
#   node 1  http://127.0.0.1:7701   (demo data imported here)
#   node 2  http://127.0.0.1:7702   (peer: node 1)
#   node 3  http://127.0.0.1:7703   (peers: node 2, node 1)
#
# Each node serves the web app from web/ at /. The demo policy is written to
# web/default-policy.json, so the app trusts the demo attestors.
# Data and logs go to a temporary directory, removed on exit unless --keep.
# Prints "READY" once node 3 holds every record of node 1. Ctrl-C (or SIGTERM
# from a script) stops all nodes. Nodes also exit within 1 s if this script is
# killed with SIGKILL.
#
# Usage: tests/live/run.sh [--size small|medium|large] [--keep] [--no-build] [--release]
# Environment: PERCH_NODE_BIN (binary to use), PERCH_PORTS (default "7701 7702 7703").
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SIZE=small
KEEP=0
BUILD=1
PROFILE=debug
while [[ $# -gt 0 ]]; do
  case "$1" in
    --size) SIZE="$2"; shift 2 ;;
    --keep) KEEP=1; shift ;;
    --no-build) BUILD=0; shift ;;
    --release) PROFILE=release; shift ;;
    -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

read -r -a PORTS <<< "${PERCH_PORTS:-7701 7702 7703}"
if [[ ${#PORTS[@]} -ne 3 ]]; then
  echo "PERCH_PORTS must list 3 ports" >&2
  exit 2
fi

BIN="${PERCH_NODE_BIN:-$ROOT/target/$PROFILE/perch-node}"
if [[ $BUILD -eq 1 && -z "${PERCH_NODE_BIN:-}" ]]; then
  if [[ $PROFILE == release ]]; then
    (cd "$ROOT" && cargo build -q -p perch-node --release)
  else
    (cd "$ROOT" && cargo build -q -p perch-node)
  fi
fi
[[ -x "$BIN" ]] || { echo "perch-node binary not found: $BIN" >&2; exit 1; }

for p in "${PORTS[@]}"; do
  if (exec 3<>"/dev/tcp/127.0.0.1/$p") 2>/dev/null; then
    echo "port $p is already in use" >&2
    exit 1
  fi
done

DATA="$(mktemp -d "${TMPDIR:-/tmp}/perch-live.XXXXXX")"
PIDS=()

cleanup() {
  trap - INT TERM EXIT
  echo
  echo "stopping nodes"
  for pid in "${PIDS[@]:-}"; do
    [[ -n "$pid" ]] && kill -TERM "$pid" 2>/dev/null || true
  done
  for pid in "${PIDS[@]:-}"; do
    [[ -n "$pid" ]] && wait "$pid" 2>/dev/null || true
  done
  if [[ $KEEP -eq 1 ]]; then
    echo "data and logs kept in $DATA"
  else
    rm -rf "$DATA"
  fi
  exit "${1:-0}"
}
trap cleanup INT TERM EXIT

"$BIN" demo --size "$SIZE" --out "$DATA/demo.jsonl" --policy-out "$DATA/policy.json" --manifest-out "$DATA/manifest.json"
mkdir -p "$ROOT/web"
cp "$DATA/policy.json" "$ROOT/web/default-policy.json"

url() { echo "http://127.0.0.1:$1"; }
ws() { echo "ws://127.0.0.1:$1/v1/sync"; }

start() {
  local n="$1"; shift
  local port="${PORTS[$((n - 1))]}"
  PERCH_EXIT_WITH_PID=$$ RUST_LOG="${RUST_LOG:-info}" "$BIN" serve \
    --listen "127.0.0.1:$port" \
    --data "$DATA/node$n" \
    --policy "$DATA/policy.json" \
    --web "$ROOT/web" \
    "$@" > "$DATA/node$n.out" 2> "$DATA/node$n.log" &
  PIDS+=("$!")
}

get() {
  # Prints the body of a GET request, or nothing on failure.
  local port="$1" path="$2"
  if command -v curl > /dev/null; then
    curl -fsS "http://127.0.0.1:$port$path" 2>/dev/null || true
  else
    exec 3<>"/dev/tcp/127.0.0.1/$port" || return 0
    printf 'GET %s HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n' "$path" >&3
    sed '1,/^\r$/d' <&3
    exec 3>&-
  fi
}

records() {
  get "$1" /v1/status | sed -n 's/.*"records":\([0-9]*\).*/\1/p'
}

wait_up() {
  local port="$1"
  for _ in $(seq 1 300); do
    [[ -n "$(get "$port" /v1/status)" ]] && return 0
    sleep 0.2
  done
  echo "node on port $port did not start; see $DATA" >&2
  exit 1
}

start 1 --import "$DATA/demo.jsonl"
wait_up "${PORTS[0]}"
start 2 --peer "$(ws "${PORTS[0]}")"
start 3 --peer "$(ws "${PORTS[1]}")" --peer "$(ws "${PORTS[0]}")"
wait_up "${PORTS[1]}"
wait_up "${PORTS[2]}"

want="$(records "${PORTS[0]}")"
for _ in $(seq 1 600); do
  if [[ "$(records "${PORTS[1]}")" == "$want" && "$(records "${PORTS[2]}")" == "$want" ]]; then
    break
  fi
  sleep 0.2
done

if [[ ! -f "$ROOT/web/pkg/perch_wasm_bg.wasm" ]]; then
  echo "note: web/pkg is missing; build the wasm client with scripts/build-wasm.sh"
fi

cat <<EOF

Perch local network ($SIZE demo, $want records on node 1)
  node 1  $(url "${PORTS[0]}")   sync $(ws "${PORTS[0]}")
  node 2  $(url "${PORTS[1]}")   sync $(ws "${PORTS[1]}")
  node 3  $(url "${PORTS[2]}")   sync $(ws "${PORTS[2]}")
  data    $DATA
  manifest $DATA/manifest.json
  policy   $ROOT/web/default-policy.json
Press Ctrl-C to stop.
READY
EOF

# Exit when any node exits, or on Ctrl-C.
wait -n "${PIDS[@]}" || true
echo "a node exited; see logs in $DATA" >&2
KEEP=1
cleanup 1
