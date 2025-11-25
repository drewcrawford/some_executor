const CDP = require('chrome-remote-interface');

async function attachToTarget(targetId) {
  try {
    const client = await CDP({port: 9222, target: targetId});
    const {Log, Runtime} = client;

    await Log.enable();
    await Runtime.enable();

    Log.entryAdded((params) => {
      const {entry} = params;
      console.log(`[WORKER:LOG:${entry.level}] ${entry.text}`);
    });

    Runtime.consoleAPICalled((params) => {
      const args = params.args.map(arg => arg.value || arg.description || '').join(' ');
      console.log(`[WORKER:CONSOLE:${params.type}] ${args}`);
    });

    console.error(`CDP: Attached to worker ${targetId}`);
  } catch (err) {
    console.error(`CDP: Failed to attach to worker ${targetId}: ${err.message}`);
  }
}

async function captureLogs() {
  // Retry connection forever - CI timeout will protect us
  let client;
  let attempt = 0;
  while (!client) {
    try {
      client = await CDP({port: 9222});
    } catch (err) {
      attempt++;
      if (attempt % 10 === 0) {
        console.error(`CDP: Waiting for Chrome (attempt ${attempt})...`);
      }
      await new Promise(r => setTimeout(r, 1000));
    }
  }

  const {Log, Runtime, Target} = client;

  // Enable logging on main page
  await Log.enable();
  await Runtime.enable();

  Log.entryAdded((params) => {
    const {entry} = params;
    console.log(`[LOG:${entry.level}] ${entry.text}`);
  });

  Runtime.consoleAPICalled((params) => {
    const args = params.args.map(arg => arg.value || arg.description || '').join(' ');
    console.log(`[CONSOLE:${params.type}] ${args}`);
  });

  // Listen for new targets (workers)
  await Target.setDiscoverTargets({discover: true});

  Target.targetCreated(async (params) => {
    const {targetInfo} = params;
    console.error(`CDP: New target: ${targetInfo.type} - ${targetInfo.targetId}`);
    if (targetInfo.type === 'worker' || targetInfo.type === 'service_worker' || targetInfo.type === 'shared_worker') {
      await attachToTarget(targetInfo.targetId);
    }
  });

  // Also check existing targets
  const {targetInfos} = await Target.getTargets();
  for (const info of targetInfos) {
    console.error(`CDP: Existing target: ${info.type} - ${info.targetId}`);
    if (info.type === 'worker' || info.type === 'service_worker' || info.type === 'shared_worker') {
      await attachToTarget(info.targetId);
    }
  }

  console.error('CDP: Connected and listening for logs (including workers)...');
}

captureLogs();
