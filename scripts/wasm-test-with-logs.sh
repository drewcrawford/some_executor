#!/bin/bash
#
# wasm-test-with-logs.sh - Run WASM tests with Chrome DevTools Protocol log capture
#
# Usage:
#   ./wasm-test-with-logs.sh [options] [-- command args...]
#
# Options:
#   -h, --help     Show this help message
#   -q, --quiet    Suppress status messages (only show WASM: prefixed output)
#   -v, --verbose  Show all CDP connection details
#
# Examples:
#   ./wasm-test-with-logs.sh
#   ./wasm-test-with-logs.sh -- cargo test --target wasm32-unknown-unknown
#   ./wasm-test-with-logs.sh -q -- wasm-pack test --headless --chrome
#
# Requirements:
#   - Node.js and npm
#   - Chrome/Chromium with chromedriver
#   - webdriver.json in the working directory (see below)
#
# webdriver.json format (place in your project root):
#   {
#     "goog:chromeOptions": {
#       "args": [
#         "--headless=new",
#         "--remote-debugging-port=9222"
#       ]
#     }
#   }
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VERBOSE=0
QUIET=0

# Default command if none specified
DEFAULT_CMD=(
    cargo +nightly test
    --target=wasm32-unknown-unknown
    --lib
    -- --nocapture
)

usage() {
    head -40 "$0" | grep '^#' | cut -c3-
    exit 0
}

log() {
    if [ "$QUIET" -eq 0 ]; then
        echo "WASM: $*" >&2
    fi
}

log_verbose() {
    if [ "$VERBOSE" -eq 1 ]; then
        echo "WASM: $*" >&2
    fi
}

# Parse arguments
CMD=()
while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            usage
            ;;
        -q|--quiet)
            QUIET=1
            shift
            ;;
        -v|--verbose)
            VERBOSE=1
            shift
            ;;
        --)
            shift
            CMD=("$@")
            break
            ;;
        *)
            CMD+=("$1")
            shift
            ;;
    esac
done

# Use default command if none provided
if [ ${#CMD[@]} -eq 0 ]; then
    CMD=("${DEFAULT_CMD[@]}")
fi

# Check for webdriver.json
if [ ! -f "webdriver.json" ]; then
    echo "WASM:ERROR: webdriver.json not found in $(pwd)" >&2
    echo "WASM:ERROR: Create it with:" >&2
    echo '{
  "goog:chromeOptions": {
    "args": [
      "--headless=new",
      "--remote-debugging-port=9222"
    ]
  }
}' >&2
    exit 1
fi

# Check for node
if ! command -v node &> /dev/null; then
    echo "WASM:ERROR: node not found. Install Node.js to capture logs." >&2
    exit 1
fi

# Install CDP dependency if needed
if [ ! -d "$SCRIPT_DIR/node_modules/chrome-remote-interface" ]; then
    log "Installing chrome-remote-interface..."
    (cd "$SCRIPT_DIR" && npm install --silent chrome-remote-interface) >&2
fi

log "Starting CDP log capture..."
log_verbose "Command: ${CMD[*]}"

# Start CDP log capture in background
node "$SCRIPT_DIR/capture-logs.js" &
CDP_PID=$!

# Cleanup on exit
cleanup() {
    if [ -n "$CDP_PID" ]; then
        kill $CDP_PID 2>/dev/null || true
        wait $CDP_PID 2>/dev/null || true
    fi
}
trap cleanup EXIT

# Run the test command with stderr merged to stdout
log "Running: ${CMD[*]}"
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER='wasm-bindgen-test-runner' \
    "${CMD[@]}" 2>&1

EXIT_CODE=$?
log "Command exited with code $EXIT_CODE"
exit $EXIT_CODE
