const CDP = require('chrome-remote-interface');

async function captureLogs() {
  // Retry connection until Chrome is ready
  let client;
  for (let i = 0; i < 30; i++) {
    try {
      client = await CDP({port: 9222});
      break;
    } catch (err) {
      await new Promise(r => setTimeout(r, 500));
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
