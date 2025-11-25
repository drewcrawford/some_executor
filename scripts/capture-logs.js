const CDP = require('chrome-remote-interface');

async function captureLogs() {
  // Retry connection until Chrome is ready
  // 120 retries × 1000ms = 2 minutes max wait (cargo build can take 30+ seconds)
  let client;
  for (let i = 0; i < 120; i++) {
    try {
      client = await CDP({port: 9222});
      break;
    } catch (err) {
      if (i % 10 === 0) {
        console.error(`CDP: Waiting for Chrome (attempt ${i + 1}/120)...`);
      }
      await new Promise(r => setTimeout(r, 1000));
    }
  }

  if (!client) {
    console.error('Failed to connect to Chrome on port 9222');
    process.exit(1);
  }

  const {Log, Runtime} = client;

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

  console.error('CDP: Connected and listening for logs...');
}

captureLogs();
