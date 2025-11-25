#!/bin/bash

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Install chrome-remote-interface if needed
cd "$SCRIPT_DIR"
if [ ! -d "node_modules/chrome-remote-interface" ]; then
    npm install chrome-remote-interface
fi
cd - > /dev/null

# Start a background job to check what's listening on 9222
(
    sleep 5
    echo "=== Checking ports ==="
    # Find chromedriver port
    DRIVER_PORT=$(lsof -i -P | grep chromedri | grep LISTEN | head -1 | awk '{print $9}' | cut -d: -f2)
    echo "Chromedriver port: $DRIVER_PORT"
    if [ -n "$DRIVER_PORT" ]; then
        echo "Querying chromedriver sessions..."
        curl -s http://localhost:$DRIVER_PORT/sessions 2>/dev/null | head -5
    fi
    # Check what ports Chrome is using
    echo "Chrome debug ports:"
    lsof -i -P | grep "Google Chrome" | grep LISTEN || echo "  none"
    echo "=== End port check ==="
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
