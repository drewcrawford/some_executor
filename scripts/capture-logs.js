#!/usr/bin/env node
//
// capture-logs.js - Capture Chrome DevTools Protocol logs from WASM tests
//

const CDP = require('chrome-remote-interface');

const PORT = 9222;
const VERBOSE = process.argv.includes('--verbose');

function log(msg) {
  console.log(`WASM:CDP: ${msg}`);
}

function logVerbose(msg) {
  if (VERBOSE) {
    console.log(`WASM:CDP: ${msg}`);
  }
}

function outputLog(type, level, text) {
  console.log(`WASM:${type}:${level}: ${text}`);
}

async function replayBufferedMessages(Runtime) {
  try {
    const {result} = await Runtime.evaluate({
      expression: `JSON.stringify(window.__cdp_console_buffer__ || [])`,
      returnByValue: true
    });
    if (result.value) {
      const buffered = JSON.parse(result.value);
      if (buffered.length > 0) {
        log(`Replaying ${buffered.length} buffered message(s)`);
        for (const msg of buffered) {
          outputLog('CONSOLE', msg.type.toUpperCase(), msg.args.join(' '));
        }
        // Clear buffer
        await Runtime.evaluate({ expression: `window.__cdp_console_buffer__ = [];` });
      }
    }
  } catch (e) {
    // Ignore - page might not have the buffer
  }
}

async function setupRuntimeLogging(Runtime, label) {
  Runtime.consoleAPICalled((params) => {
    const args = params.args.map(arg => arg.value || arg.description || '').join(' ');
    outputLog('CONSOLE', params.type.toUpperCase(), args);
  });

  Runtime.exceptionThrown(({exceptionDetails}) => {
    const text = exceptionDetails.exception?.description
      || exceptionDetails.text
      || JSON.stringify(exceptionDetails);
    outputLog('EXCEPTION', 'ERROR', text);
  });
}

async function attachToTarget(Target, targetId, targetType) {
  try {
    const client = await CDP({port: PORT, target: targetId});
    const {Runtime, Log, Network} = client;

    // Enable and set up listeners immediately
    await Runtime.enable();
    setupRuntimeLogging(Runtime, targetType);

    // Replay any buffered messages
    await replayBufferedMessages(Runtime);

    // Optional extras
    try {
      await Log.enable();
      Log.entryAdded(({entry}) => {
        if (entry.level === 'verbose' && !VERBOSE) return;
        outputLog('LOG', entry.level.toUpperCase(), entry.text);
      });
    } catch (e) {}

    try {
      await Network.enable();

      const pendingRequests = new Map();

      // Track requests
      Network.requestWillBeSent && Network.requestWillBeSent(({requestId, request, type}) => {
        const {url} = request;
        if (url.includes('favicon') || url.startsWith('data:')) return;
        if (url.endsWith('.js') || url.endsWith('.wasm') || type === 'Script') {
          pendingRequests.set(requestId, {url, type});
        }
      });

      // When response received, note the mime type
      Network.responseReceived && Network.responseReceived(({requestId, response}) => {
        const pending = pendingRequests.get(requestId);
        if (pending) {
          pending.mimeType = response.mimeType;
          pending.status = response.status;
        }
      });

      // When loading finishes, fetch and log the body
      Network.loadingFinished && Network.loadingFinished(async ({requestId}) => {
        const pending = pendingRequests.get(requestId);
        if (!pending) return;
        pendingRequests.delete(requestId);

        // Skip binary WASM files - just note they were loaded
        if (pending.url.endsWith('.wasm') || pending.mimeType === 'application/wasm') {
          outputLog('RESOURCE', 'LOADED', `${pending.url} (binary wasm, not dumped)`);
          return;
        }

        try {
          const {body, base64Encoded} = await Network.getResponseBody({requestId});
          const content = base64Encoded ? Buffer.from(body, 'base64').toString('utf8') : body;

          outputLog('RESOURCE', 'START', `=== ${pending.url} (${pending.mimeType}) ===`);
          // Print content, limit to 100000 chars for JS files
          const lines = content.substring(0, 100000).split('\n');
          for (const line of lines) {
            console.log(`WASM:RESOURCE:CONTENT: ${line}`);
          }
          if (content.length > 100000) {
            console.log(`WASM:RESOURCE:CONTENT: ... truncated (${content.length} total chars)`);
          }
          outputLog('RESOURCE', 'END', `=== ${pending.url} ===`);
        } catch (e) {
          logVerbose(`Could not get body for ${pending.url}: ${e.message}`);
        }
      });
    } catch (e) {}

    logVerbose(`Attached to ${targetType}`);
  } catch (err) {
    logVerbose(`Failed to attach to ${targetId}: ${err.message}`);
  }
}

async function captureLogs() {
  let browser;
  let attempt = 0;

  // Retry connection forever
  while (!browser) {
    try {
      browser = await CDP({port: PORT});
    } catch (err) {
      attempt++;
      if (attempt % 30 === 0) {
        log(`Waiting for Chrome on port ${PORT} (attempt ${attempt})...`);
      }
      await new Promise(r => setTimeout(r, 1000));
    }
  }

  log('Connected');

  const {Target, Page, Runtime} = browser;

  // Set up target discovery FIRST - this is the critical path
  await Target.setDiscoverTargets({discover: true});

  const attachedTargets = new Set();
  const attachIfNeeded = async (targetId, targetType) => {
    if (attachedTargets.has(targetId)) return;
    attachedTargets.add(targetId);
    await attachToTarget(Target, targetId, targetType);
  };

  // Listen for new targets
  Target.targetCreated(async ({targetInfo}) => {
    const {type, targetId} = targetInfo;
    if (type === 'page' || type === 'worker' || type === 'iframe') {
      await attachIfNeeded(targetId, type);
    }
  });

  // Attach to existing targets immediately
  const {targetInfos} = await Target.getTargets();
  const pageTargets = targetInfos.filter(t =>
    t.type === 'page' || t.type === 'worker' || t.type === 'iframe'
  );

  // Attach in parallel for speed
  await Promise.all(pageTargets.map(t => attachIfNeeded(t.targetId, t.type)));

  // Install early interceptor for future pages
  try {
    await Page.enable();
    await Page.addScriptToEvaluateOnNewDocument({
      source: `
        if (!window.__cdp_console_injected__) {
          window.__cdp_console_buffer__ = [];
          window.__cdp_console_injected__ = true;
          ['log', 'warn', 'error', 'info', 'debug'].forEach(m => {
            const orig = console[m].bind(console);
            console[m] = function(...args) {
              window.__cdp_console_buffer__.push({
                type: m,
                args: args.map(a => { try { return typeof a === 'object' ? JSON.stringify(a) : String(a); } catch(e) { return String(a); } })
              });
              return orig(...args);
            };
          });
        }
      `
    });
  } catch (e) {
    logVerbose(`Early interceptor failed: ${e.message}`);
  }

  // Also set up browser-level runtime (sometimes catches things)
  try {
    await Runtime.enable();
    setupRuntimeLogging(Runtime, 'browser');
  } catch (e) {}

  log('Listening');

  // Periodically check for buffered messages on known targets
  setInterval(async () => {
    for (const targetId of attachedTargets) {
      try {
        const client = await CDP({port: PORT, target: targetId});
        await replayBufferedMessages(client.Runtime);
      } catch (e) {}
    }
  }, 2000);
}

captureLogs().catch(err => {
  console.error(`WASM:CDP:ERROR: ${err.message}`);
  process.exit(1);
});
