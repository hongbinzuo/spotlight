import test from "node:test";
import assert from "node:assert/strict";

import { evaluateRecoveryClaimGuard } from "./recovery-claim-guard.mjs";

test("evaluateRecoveryClaimGuard disables auto agents when paused backlog exists", async () => {
  const calls = [];
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    calls.push({ url: String(url), method: init?.method || "GET" });
    if (!init?.method) {
      return {
        ok: true,
        json: async () => ({
          tasks: [
            { status: "PAUSED" },
            { status: "OPEN" },
            { status: "RUNNING" }
          ],
          agents: [
            { id: "agent-a", name: "Agent A", auto_mode: true },
            { id: "agent-b", name: "Agent B", auto_mode: false }
          ]
        })
      };
    }
    return {
      ok: true,
      json: async () => ({ ok: true })
    };
  };

  try {
    const result = await evaluateRecoveryClaimGuard("http://example.test", true);
    assert.equal(result.shouldDisable, true);
    assert.equal(result.pausedCount, 1);
    assert.equal(result.toggled.length, 1);
    assert.equal(result.toggled[0].id, "agent-a");
    assert.equal(calls.filter((item) => item.method === "POST").length, 1);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("evaluateRecoveryClaimGuard leaves agents untouched when no paused tasks remain", async () => {
  const calls = [];
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    calls.push({ url: String(url), method: init?.method || "GET" });
    return {
      ok: true,
      json: async () => ({
        tasks: [
          { status: "OPEN" },
          { status: "RUNNING" }
        ],
        agents: [
          { id: "agent-a", name: "Agent A", auto_mode: true }
        ]
      })
    };
  };

  try {
    const result = await evaluateRecoveryClaimGuard("http://example.test", true);
    assert.equal(result.shouldDisable, false);
    assert.equal(result.toggled.length, 0);
    assert.equal(calls.filter((item) => item.method === "POST").length, 0);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
