#!/bin/bash

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=== Environment ==="
echo "Working directory: $(pwd)"
echo "Script directory: $SCRIPT_DIR"
echo "Repo directory: $REPO_DIR"
echo "webdriver.json exists: $(test -f webdriver.json && echo 'yes in cwd' || echo 'no in cwd')"
echo "webdriver.json in repo: $(test -f "$REPO_DIR/webdriver.json" && echo 'yes' || echo 'no')"
if [ -f "$REPO_DIR/webdriver.json" ]; then
    echo "webdriver.json contents:"
    cat "$REPO_DIR/webdriver.json"
fi
echo "node available: $(which node 2>/dev/null || echo 'NOT FOUND')"
echo "npm available: $(which npm 2>/dev/null || echo 'NOT FOUND')"
echo "==================="

# Start a background job to check ports and poll console logs periodically
(
    for i in 5 10 15 20 25 30 40 50 60; do
        sleep $((i - ${PREV_I:-0}))
        PREV_I=$i
        echo "=== Check at ${i}s ==="
        echo "Port 9222:"
        VERSION_INFO=$(curl -s http://localhost:9222/json/version 2>/dev/null)
        if [ -n "$VERSION_INFO" ]; then
            echo "$VERSION_INFO"
            # Try to get open pages/targets
            echo "--- Open targets ---"
            curl -s http://localhost:9222/json/list 2>/dev/null | head -50
        else
            echo "  not responding"
        fi
    done
) &
CHECK_PID=$!

# Try to start CDP log capture if node is available
CDP_PID=""
if which node >/dev/null 2>&1; then
    echo "Node found, attempting CDP log capture..."
    cd "$SCRIPT_DIR"
    if [ ! -d "node_modules/chrome-remote-interface" ]; then
        npm install chrome-remote-interface 2>&1 || echo "npm install failed"
    fi
    cd - > /dev/null
    node "$SCRIPT_DIR/capture-logs.js" &
    CDP_PID=$!
else
    echo "Node not found, skipping CDP log capture"
fi

# Cleanup on exit
cleanup() {
    [ -n "$CHECK_PID" ] && kill $CHECK_PID 2>/dev/null
    [ -n "$CDP_PID" ] && kill $CDP_PID 2>/dev/null
}
trap cleanup EXIT

# Run the test from repo directory (so webdriver.json is found)
cd "$REPO_DIR"
CHROMEDRIVER=$(which chromedriver) \
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER='wasm-bindgen-test-runner' \
cargo +nightly test --target=wasm32-unknown-unknown --lib -- --nocapture
