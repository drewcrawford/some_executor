const CDP = require('chrome-remote-interface');

async function setupLogging(client, label) {
  const {Log, Runtime, Network, Page} = client;

  await Log.enable();
  await Runtime.enable();

  // Try to enable Network and Page (may not be available on all targets)
  try { await Network.enable(); } catch (e) { /* ignore */ }
  try { await Page.enable(); } catch (e) { /* ignore */ }

  Log.entryAdded((params) => {
    const {entry} = params;
    console.log(`[${label}:LOG:${entry.level}] ${entry.text}`);
  });

  Runtime.consoleAPICalled((params) => {
    const args = params.args.map(arg => arg.value || arg.description || '').join(' ');
    console.log(`[${label}:CONSOLE:${params.type}] ${args}`);
  });

  // Catch exceptions
  Runtime.exceptionThrown((params) => {
    const details = params.exceptionDetails;
    const text = details.exception?.description || details.text || JSON.stringify(details);
    console.log(`[${label}:EXCEPTION] ${text}`);
  });

  // Network failures
  Network.requestFailed && Network.requestFailed((params) => {
    console.log(`[${label}:NET:FAILED] ${params.request?.url} - ${params.errorText}`);
  });

  // Network responses (for 4xx/5xx)
  Network.responseReceived && Network.responseReceived((params) => {
    const {response} = params;
    if (response.status >= 400) {
      console.log(`[${label}:NET:${response.status}] ${response.url}`);
    }
  });

  // Page crashes
  Page.javascriptDialogOpening && Page.javascriptDialogOpening((params) => {
    console.log(`[${label}:DIALOG] ${params.type}: ${params.message}`);
  });

  console.error(`CDP: Logging enabled for ${label}`);
}

async function captureLogs() {
  // Retry connection forever - CI timeout will protect us
  let browser;
  let attempt = 0;
  while (!browser) {
    try {
      browser = await CDP({port: 9222});
    } catch (err) {
      attempt++;
      if (attempt % 10 === 0) {
        console.error(`CDP: Waiting for Chrome (attempt ${attempt})...`);
      }
      await new Promise(r => setTimeout(r, 1000));
    }
  }

  // Set up logging on browser-level connection too
  await setupLogging(browser, 'browser');

  const {Target} = browser;

  // Discover all targets
  await Target.setDiscoverTargets({discover: true});

  // Attach to each page target we find
  const attachToPage = async (targetId, targetType) => {
    try {
      // Create a new CDP session for this target
      const {sessionId} = await Target.attachToTarget({targetId, flatten: true});
      console.error(`CDP: Attached to ${targetType} ${targetId} (session: ${sessionId})`);

      // Connect to this specific target
      const client = await CDP({port: 9222, target: targetId});
      await setupLogging(client, targetType);
    } catch (err) {
      console.error(`CDP: Failed to attach to ${targetId}: ${err.message}`);
    }
  };

  // Listen for new targets
  Target.targetCreated(async ({targetInfo}) => {
    console.error(`CDP: New target: ${targetInfo.type} - ${targetInfo.url || targetInfo.targetId}`);
    if (targetInfo.type === 'page' || targetInfo.type === 'worker' || targetInfo.type === 'iframe') {
      await attachToPage(targetInfo.targetId, targetInfo.type);
    }
  });

  // Attach to existing targets
  const {targetInfos} = await Target.getTargets();
  for (const info of targetInfos) {
    console.error(`CDP: Existing target: ${info.type} - ${info.url || info.targetId}`);
    if (info.type === 'page' || info.type === 'worker' || info.type === 'iframe') {
      await attachToPage(info.targetId, info.type);
    }
  }

  console.error('CDP: Setup complete, listening for all targets...');
}

captureLogs().catch(err => {
  console.error('CDP: Fatal error:', err);
  process.exit(1);
});
