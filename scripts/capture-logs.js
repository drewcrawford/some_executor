const CDP = require('chrome-remote-interface');

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
