#!/bin/bash

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Install chrome-remote-interface if needed
cd "$SCRIPT_DIR"
if [ ! -d "node_modules/chrome-remote-interface" ]; then
    npm install chrome-remote-interface
fi
cd - > /dev/null

# Start a background job to check ports periodically
(
    for i in 5 10 20 30; do
        sleep $i
        echo "=== Port check at ${i}s ==="
        echo "Port 9222:"
        curl -s http://localhost:9222/json/version 2>/dev/null && echo "" || echo "  not responding"
        echo "Processes with 'chrome' in name:"
        ps aux 2>/dev/null | grep -i chrome | grep -v grep | head -5 || echo "  none found"
    done
) &
CHECK_PID=$!

# Start CDP log capture (will retry until Chrome is up)
node "$SCRIPT_DIR/capture-logs.js" &
CDP_PID=$!

# Cleanup on exit
trap "kill $CDP_PID $CHECK_PID 2>/dev/null" EXIT

# Run the test
CHROMEDRIVER=$(which chromedriver) \
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER='wasm-bindgen-test-runner' \
cargo +nightly test --target=wasm32-unknown-unknown --lib -- --nocapture
