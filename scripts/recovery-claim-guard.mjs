import { pathToFileURL } from "node:url";

const DEFAULT_SERVER = "http://127.0.0.1:3000";

function hasFlag(argv, flag) {
  return argv.includes(flag);
}

function optionValue(argv, name, fallback) {
  const prefix = `${name}=`;
  const item = argv.find((entry) => entry.startsWith(prefix));
  return item ? item.slice(prefix.length) : fallback;
}

async function requestJson(url, init) {
  const response = await fetch(url, init);
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`${response.status} ${response.statusText}: ${text}`);
  }
  return response.json();
}

export async function evaluateRecoveryClaimGuard(serverBase = DEFAULT_SERVER, apply = false) {
  const board = await requestJson(`${serverBase}/api/v1/board`);
  const pausedCount = (board.tasks || []).filter((task) => task.status === "PAUSED").length;
  const openCount = (board.tasks || []).filter((task) => task.status === "OPEN").length;
  const runningCount = (board.tasks || []).filter((task) => task.status === "RUNNING").length;
  const autoAgents = (board.agents || []).filter((agent) => agent.auto_mode);
  const shouldDisable = pausedCount > 0;
  const toggled = [];

  if (apply && shouldDisable) {
    for (const agent of autoAgents) {
      await requestJson(`${serverBase}/api/v1/agents/${agent.id}/auto-mode/toggle`, {
        method: "POST"
      });
      toggled.push({
        id: agent.id,
        name: agent.name,
        action: "disabled_auto_mode_due_to_recovery_backlog"
      });
    }
  }

  return {
    serverBase,
    pausedCount,
    openCount,
    runningCount,
    shouldDisable,
    autoAgentCount: autoAgents.length,
    toggled
  };
}

async function main(argv = process.argv.slice(2)) {
  const serverBase = optionValue(argv, "--server", DEFAULT_SERVER);
  const apply = hasFlag(argv, "--apply");
  const result = await evaluateRecoveryClaimGuard(serverBase, apply);
  console.log(JSON.stringify(result, null, 2));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.stack || String(error));
    process.exitCode = 1;
  });
}
