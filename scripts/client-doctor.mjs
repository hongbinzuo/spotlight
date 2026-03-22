import { spawnSync } from "node:child_process";

const BOARD_BYTES_WARN = 1_000_000;
const BOARD_BYTES_FAIL = 4_000_000;
const BOARD_LATENCY_WARN_MS = 1_500;
const BOARD_LATENCY_FAIL_MS = 4_000;

function hasFlag(argv, flag) {
  return argv.includes(flag);
}

function optionValue(argv, name, fallback) {
  const prefix = `${name}=`;
  const item = argv.find((entry) => entry.startsWith(prefix));
  return item ? item.slice(prefix.length) : fallback;
}

function summarize(text, limit = 140) {
  const compact = String(text || "").replace(/\s+/g, " ").trim();
  if (!compact) return "";
  return compact.length <= limit ? compact : `${compact.slice(0, limit)}...`;
}

function safeArray(value) {
  return Array.isArray(value) ? value : [];
}

function parseTaskVersion(task) {
  const title = String(task?.title || "");
  const match = title.match(/^\[(\d+\.\d+\.\d+)\]/);
  return match ? match[1] : null;
}

function taskStatusCounts(tasks) {
  return safeArray(tasks).reduce((counts, task) => {
    const status = String(task?.status || "UNKNOWN");
    counts[status] = (counts[status] || 0) + 1;
    return counts;
  }, {});
}

const AUTO_ACTIVITY_KINDS = [
  "task.auto_claimed",
  "task.auto_started",
  "task.auto_resumed",
  "task.auto_retry_queued",
  "task.watchdog_recovered"
];

function taskStateSnapshot(task) {
  return task?.state_snapshot || {};
}

function taskStateReason(task) {
  return summarize(taskStateSnapshot(task)?.reason, 180);
}

function taskStateEvidence(task) {
  return safeArray(taskStateSnapshot(task)?.evidence).filter(
    (item) => typeof item === "string" && item.trim()
  );
}

function hasTaskStateSnapshot(task) {
  return Boolean(taskStateReason(task)) && taskStateEvidence(task).length > 0;
}

function clamp01(value) {
  return Math.max(0, Math.min(1, Number(value) || 0));
}

function ratio(numerator, denominator) {
  return denominator > 0 ? numerator / denominator : 1;
}

function percent(value) {
  return Math.round(clamp01(value) * 100);
}

function taskAutoManaged(task, autoAgentIds = new Set()) {
  const claimedBy = String(task?.claimed_by || "");
  if (claimedBy && autoAgentIds.has(claimedBy)) {
    return true;
  }
  return safeArray(task?.activities).some((item) => AUTO_ACTIVITY_KINDS.includes(item?.kind));
}

function taskRecoveryConsistent(task) {
  if (!hasTaskStateSnapshot(task)) {
    return false;
  }
  if (task?.status === "RUNNING") {
    return Boolean(task?.runtime?.active_turn_id) || !taskStateSnapshot(task)?.needs_attention;
  }
  return !task?.runtime?.active_turn_id;
}

function taskHasEvolutionEvidence(task, summaryDigestIds = new Set()) {
  if (task?.status !== "DONE") {
    return false;
  }
  if (summaryDigestIds.has(String(task?.id || ""))) {
    return true;
  }
  return taskStateEvidence(task).some((item) =>
    item.startsWith("completion.summary:")
    || item.startsWith("last_activity:task.done@")
    || item.startsWith("last_activity:task.completed@")
  );
}

function computeAutonomyMetrics(board, summaries = []) {
  const tasks = safeArray(board?.tasks);
  const agents = safeArray(board?.agents);
  const autoAgentIds = new Set(
    agents
      .filter((agent) => agent?.auto_mode)
      .map((agent) => String(agent?.id || ""))
      .filter(Boolean)
  );
  const summaryDigestIds = new Set(
    safeArray(summaries)
      .flatMap((summary) => safeArray(summary?.recent_task_summaries))
      .map((item) => String(item?.task_id || ""))
      .filter(Boolean)
  );

  const executableTasks = tasks.filter((task) => !["DONE", "CANCELED"].includes(task?.status));
  const recoverableTasks = tasks.filter((task) => ["CLAIMED", "RUNNING", "PAUSED", "FAILED"].includes(task?.status));
  const doneTasks = tasks.filter((task) => task?.status === "DONE");
  const stateReliableCount = tasks.filter(hasTaskStateSnapshot).length;
  const autoManagedCount = executableTasks.filter((task) => taskAutoManaged(task, autoAgentIds)).length;
  const recoveryConsistentCount = recoverableTasks.filter(taskRecoveryConsistent).length;
  const evolutionCount = doneTasks.filter((task) => taskHasEvolutionEvidence(task, summaryDigestIds)).length;
  const needsAttentionCount = tasks.filter((task) => taskStateSnapshot(task)?.needs_attention).length;
  const unmanagedOpenCount = tasks.filter(
    (task) => task?.status === "OPEN" && !taskAutoManaged(task, autoAgentIds)
  ).length;

  const stateConfidence = ratio(stateReliableCount, tasks.length);
  const autoRunCoverage = ratio(autoManagedCount, executableTasks.length);
  const autoRecoveryCoverage = ratio(recoveryConsistentCount, recoverableTasks.length);
  const evolutionCoverage = ratio(evolutionCount, doneTasks.length);
  const overall = (stateConfidence + autoRunCoverage + autoRecoveryCoverage + evolutionCoverage) / 4;

  return {
    overall,
    overallPercent: percent(overall),
    counts: {
      totalTasks: tasks.length,
      executableTasks: executableTasks.length,
      recoverableTasks: recoverableTasks.length,
      doneTasks: doneTasks.length,
      needsAttention: needsAttentionCount,
      unmanagedOpen: unmanagedOpenCount
    },
    metrics: [
      {
        code: "state_confidence",
        label: "状态可信度",
        score: stateConfidence,
        percent: percent(stateConfidence),
        numerator: stateReliableCount,
        denominator: tasks.length,
        description: `${stateReliableCount}/${tasks.length} 个任务已有状态原因和证据`
      },
      {
        code: "auto_run_coverage",
        label: "自动运行覆盖",
        score: autoRunCoverage,
        percent: percent(autoRunCoverage),
        numerator: autoManagedCount,
        denominator: executableTasks.length,
        description: `${autoManagedCount}/${executableTasks.length} 个非终态任务已纳入自动推进链路`
      },
      {
        code: "auto_recovery_coverage",
        label: "自动恢复覆盖",
        score: autoRecoveryCoverage,
        percent: percent(autoRecoveryCoverage),
        numerator: recoveryConsistentCount,
        denominator: recoverableTasks.length,
        description: `${recoveryConsistentCount}/${recoverableTasks.length} 个活跃/可恢复任务状态自洽`
      },
      {
        code: "evolution_coverage",
        label: "自动进化沉淀率",
        score: evolutionCoverage,
        percent: percent(evolutionCoverage),
        numerator: evolutionCount,
        denominator: doneTasks.length,
        description: `${evolutionCount}/${doneTasks.length} 个已完成任务有摘要或记忆沉淀`
      }
    ]
  };
}

function analyzeBoard(board, options = {}) {
  const failures = [];
  const warnings = [];
  const tasks = safeArray(board?.tasks);
  const agents = safeArray(board?.agents);
  const autoAgents = agents.filter((agent) => agent?.auto_mode);
  const activeTasks = tasks.filter((task) => ["RUNNING", "CLAIMED"].includes(task?.status));
  const pendingTasks = tasks.filter((task) => ["OPEN", "PAUSED"].includes(task?.status));
  const boardBytes = Number(options.boardBytes || 0);
  const boardLatencyMs = Number(options.boardLatencyMs || 0);

  if (boardBytes >= BOARD_BYTES_FAIL) {
    failures.push({
      code: "board_payload_too_large",
      message: `看板快照过大，当前约 ${(boardBytes / (1024 * 1024)).toFixed(2)} MB，客户端轮询很容易触发超时或 Failed to fetch。`,
      details: { boardBytes }
    });
  } else if (boardBytes >= BOARD_BYTES_WARN) {
    warnings.push({
      code: "board_payload_large",
      message: `看板快照已经偏大，当前约 ${(boardBytes / (1024 * 1024)).toFixed(2)} MB，建议继续压缩。`,
      details: { boardBytes }
    });
  }

  if (boardLatencyMs >= BOARD_LATENCY_FAIL_MS) {
    failures.push({
      code: "board_latency_too_high",
      message: `看板接口耗时 ${boardLatencyMs}ms，已经超过客户端可稳定轮询的范围。`,
      details: { boardLatencyMs }
    });
  } else if (boardLatencyMs >= BOARD_LATENCY_WARN_MS) {
    warnings.push({
      code: "board_latency_high",
      message: `看板接口耗时 ${boardLatencyMs}ms，客户端轮询压力偏高。`,
      details: { boardLatencyMs }
    });
  }

  if (pendingTasks.length > 0 && autoAgents.length === 0) {
    failures.push({
      code: "no_auto_agents_for_pending_tasks",
      message: "当前仍有待处理任务，但没有任何处于自动模式的 Agent。",
      details: {
        pendingTasks: pendingTasks.length,
        taskStatus: taskStatusCounts(tasks)
      }
    });
  }

  if (pendingTasks.length > 0 && autoAgents.length > 0 && activeTasks.length === 0) {
    failures.push({
      code: "auto_agents_idle_with_pending_tasks",
      message: "存在待处理任务且自动模式 Agent 处于空闲，但系统没有任何 RUNNING / CLAIMED 任务，说明自动推进链路已经停住。",
      details: {
        pendingTasks: pendingTasks.length,
        autoAgents: autoAgents.length,
        taskStatus: taskStatusCounts(tasks)
      }
    });
  }

  for (const task of tasks) {
    const activities = safeArray(task?.activities);
    const runtimeLog = safeArray(task?.runtime?.log);
    const stateSnapshot = task?.state_snapshot || {};
    const stateReason = summarize(stateSnapshot?.reason);
    const stateEvidence = safeArray(stateSnapshot?.evidence);
    const watchdogRecoveries = activities.filter((item) => item?.kind === "task.watchdog_recovered").length;
    const threadStarted = activities.some((item) => item?.kind === "runtime.thread_started");
    const version = parseTaskVersion(task);
    const lastActivityKind = activities.at(-1)?.kind || null;
    const recoveryIncidentActive =
      lastActivityKind === "task.watchdog_recovered"
      || lastActivityKind === "task.auto_retry_queued"
      || ["OPEN", "CLAIMED", "RUNNING"].includes(task?.status);

    if (
      task?.status === "OPEN"
      && task?.claimed_by == null
      && (threadStarted || runtimeLog.length > 0 || watchdogRecoveries > 0)
    ) {
      failures.push({
        code: "task_progress_reverted_to_open",
        title: task?.title || "未命名任务",
        message: "任务已有运行痕迹，但当前又回到 OPEN，客户端会误以为它从未真正开始。",
        details: {
          status: task?.status,
          activities: activities.length,
          runtimeLog: runtimeLog.length,
          watchdogRecoveries
        }
      });
    }

    if (!stateReason || stateEvidence.length === 0) {
      failures.push({
        code: "task_state_snapshot_missing",
        title: task?.title || "unknown task",
        message: "任务缺少服务端生成的状态原因或证据快照，客户端仍需要靠 status 和日志侧推，状态评估不可信。",
        details: {
          status: task?.status,
          hasReason: Boolean(stateReason),
          evidenceCount: stateEvidence.length
        }
      });
    }

    if (stateSnapshot?.needs_attention) {
      warnings.push({
        code: "task_state_needs_attention",
        title: task?.title || "unknown task",
        message: `任务状态被服务端标记为需要复核：${stateReason || "缺少明确说明"}`,
        details: {
          status: task?.status,
          evidenceCount: stateEvidence.length
        }
      });
    }

    if (task?.status !== "RUNNING" && task?.runtime?.active_turn_id) {
      warnings.push({
        code: "task_non_running_with_active_turn",
        title: task?.title || "unknown task",
        message: "任务当前并非 RUNNING，但仍保留 active_turn_id，说明运行时状态和业务状态可能没有完全对齐。",
        details: {
          status: task?.status,
          activeTurnId: task?.runtime?.active_turn_id
        }
      });
    }

    if (watchdogRecoveries >= 2 && recoveryIncidentActive) {
      warnings.push({
        code: "task_repeated_watchdog_recovery",
        title: task?.title || "未命名任务",
        message: `任务已被 watchdog 回收 ${watchdogRecoveries} 次，存在 thread/session 恢复不稳定或状态机回退问题。`,
        details: {
          status: task?.status,
          version
        }
      });
    }

    if (version && task?.priority == null) {
      warnings.push({
        code: "versioned_task_missing_priority",
        title: task?.title || "未命名任务",
        message: "版本任务缺少优先级，客户端和自动认领很难做稳定排序。",
        details: {
          version
        }
      });
    }
  }

  return { failures, warnings };
}

function analyzeProjectSummaries(board, summaries) {
  const failures = [];
  const warnings = [];
  const tasks = safeArray(board?.tasks);
  const agents = safeArray(board?.agents);

  for (const summary of safeArray(summaries)) {
    const projectId = summary?.project_id;
    const projectTasks = tasks.filter((task) => task?.project_id === projectId);
    const pendingProjectTasks = projectTasks.filter((task) => ["OPEN", "PAUSED"].includes(task?.status));
    const totalProjectTasks = projectTasks.length;
    const summaryTaskTotal =
      Number(summary?.task_counts?.open || 0)
      + Number(summary?.task_counts?.claimed || 0)
      + Number(summary?.task_counts?.running || 0)
      + Number(summary?.task_counts?.paused || 0)
      + Number(summary?.task_counts?.done || 0)
      + Number(summary?.task_counts?.failed || 0)
      + Number(summary?.task_counts?.canceled || 0);

    if (totalProjectTasks !== summaryTaskTotal) {
      failures.push({
        code: "project_summary_task_mismatch",
        message: `项目摘要中的任务统计与看板快照不一致：${summary?.project_name || projectId}`,
        details: {
          projectId,
          boardTaskTotal: totalProjectTasks,
          summaryTaskTotal
        }
      });
    }

    if (pendingProjectTasks.length > 0 && agents.length > 0 && Number(summary?.agent_summary?.total || 0) === 0) {
      failures.push({
        code: "project_summary_missing_agents",
        message: `项目 ${summary?.project_name || projectId} 仍有待处理任务，但摘要把 Agent 总数报告为 0，监控会误判为健康。`,
        details: {
          projectId,
          pendingTasks: pendingProjectTasks.length,
          summaryAgentTotal: Number(summary?.agent_summary?.total || 0),
          boardAgentTotal: agents.length
        }
      });
    }

    if (Number(summary?.agent_summary?.busy || 0) > Number(summary?.agent_summary?.total || 0)) {
      warnings.push({
        code: "project_summary_busy_agent_overflow",
        message: `项目 ${summary?.project_name || projectId} 的忙碌 Agent 数超过总数，摘要指标异常。`,
        details: {
          projectId,
          agentSummary: summary?.agent_summary || null
        }
      });
    }
  }

  return { failures, warnings };
}

function analyzeHtml(html) {
  const failures = [];
  const warnings = [];
  const text = String(html || "");

  if (text.includes("Spotlight 0.1.0")) {
    warnings.push({
      code: "ui_static_app_version",
      message: "统一入口页仍暴露静态的 Spotlight 0.1.0 文案，容易和任务版本混淆。",
      details: {}
    });
  }

  return { failures, warnings };
}

function mergeReports(...reports) {
  return reports.reduce(
    (acc, report) => {
      acc.failures.push(...safeArray(report?.failures));
      acc.warnings.push(...safeArray(report?.warnings));
      return acc;
    },
    { failures: [], warnings: [] }
  );
}

function listProcesses() {
  if (process.platform === "win32") {
    const result = spawnSync("tasklist", ["/FO", "CSV", "/NH"], { encoding: "utf8" });
    if (result.status !== 0) {
      return [];
    }
    return result.stdout
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean)
      .map((line) => {
        const cols = line
          .replace(/^"|"$/g, "")
          .split('","');
        return { name: cols[0] || "", pid: Number(cols[1] || 0) };
      });
  }

  const result = spawnSync("ps", ["-ax", "-o", "pid=,comm="], { encoding: "utf8" });
  if (result.status !== 0) {
    return [];
  }
  return result.stdout
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const match = line.match(/^(\d+)\s+(.+)$/);
      return {
        name: match ? match[2].split("/").pop() : "",
        pid: match ? Number(match[1]) : 0
      };
    });
}

async function fetchJson(url) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`${url} -> ${response.status}`);
  }
  return response.json();
}

async function fetchText(url) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`${url} -> ${response.status}`);
  }
  return response.text();
}

async function fetchJsonText(url) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`${url} -> ${response.status}`);
  }
  const text = await response.text();
  return {
    json: JSON.parse(text),
    bytes: Buffer.byteLength(text, "utf8")
  };
}

async function runDoctor(options = {}) {
  const serverBase = options.serverBase || "http://127.0.0.1:3000";
  const webBase = options.webBase || "http://127.0.0.1:1421";
  const processes = listProcesses();
  const report = {
    generatedAt: new Date().toISOString(),
    serverBase,
    webBase,
    checks: [],
    failures: [],
    warnings: [],
    autonomy: null
  };

  const requiredProcesses = ["spotlight-server.exe", "spotlight-desktop.exe"];
  for (const name of requiredProcesses) {
    const matched = processes.filter((item) => item.name.toLowerCase() === name.toLowerCase());
    report.checks.push({
      kind: "process",
      target: name,
      ok: matched.length > 0,
      details: matched
    });
    if (!matched.length) {
      report.failures.push({
        code: "required_process_missing",
        message: `关键进程未运行：${name}`,
        details: {}
      });
    }
  }

  const endpointChecks = [
    `${serverBase}/`,
    `${serverBase}/api/v1/me`,
    `${serverBase}/api/v1/projects`,
    `${serverBase}/api/v1/board`,
    `${webBase}/`
  ];

  for (const url of endpointChecks) {
    try {
      const response = await fetch(url);
      report.checks.push({
        kind: "http",
        target: url,
        ok: response.ok,
        details: { status: response.status }
      });
      if (!response.ok) {
        report.failures.push({
          code: "endpoint_unhealthy",
          message: `接口异常：${url} -> ${response.status}`,
          details: {}
        });
      }
    } catch (error) {
      report.checks.push({
        kind: "http",
        target: url,
        ok: false,
        details: { error: String(error.message || error) }
      });
      report.failures.push({
        code: "endpoint_unreachable",
        message: `接口不可达：${url}`,
        details: { error: String(error.message || error) }
      });
    }
  }

  try {
    const boardStartedAt = Date.now();
    const { json: board, bytes: boardBytes } = await fetchJsonText(`${serverBase}/api/v1/board`);
    const boardLatencyMs = Date.now() - boardStartedAt;
    report.checks.push({
      kind: "board",
      target: `${serverBase}/api/v1/board`,
      ok: true,
      details: {
        boardBytes,
        boardLatencyMs,
        taskCount: safeArray(board?.tasks).length,
        agentCount: safeArray(board?.agents).length
      }
    });

    const boardReport = analyzeBoard(board, { boardBytes, boardLatencyMs });
    report.failures.push(...boardReport.failures);
    report.warnings.push(...boardReport.warnings);

    const projects = safeArray(await fetchJson(`${serverBase}/api/v1/projects`));
    let summaries = [];
    if (projects.length > 0) {
      for (const project of projects) {
        const summary = await fetchJson(`${serverBase}/api/v1/projects/${project.id}/summary`);
        summaries.push(summary);
        report.checks.push({
          kind: "summary",
          target: `${serverBase}/api/v1/projects/${project.id}/summary`,
          ok: Boolean(summary?.project_id),
          details: {
            projectName: project?.name || null,
            taskCounts: summary?.task_counts || null,
            agentSummary: summary?.agent_summary || null
          }
        });
      }

      const summaryReport = analyzeProjectSummaries(board, summaries);
      report.failures.push(...summaryReport.failures);
      report.warnings.push(...summaryReport.warnings);
    } else {
      report.warnings.push({
        code: "no_projects",
        message: "当前没有可用项目，无法继续验证摘要链路。",
        details: {}
      });
    }
    report.autonomy = computeAutonomyMetrics(board, summaries);
  } catch (error) {
    report.failures.push({
      code: "board_or_summary_check_failed",
      message: "读取 board / summary 失败",
      details: { error: String(error.message || error) }
    });
  }

  try {
    const html = await fetchText(`${serverBase}/`);
    const htmlReport = analyzeHtml(html);
    report.failures.push(...htmlReport.failures);
    report.warnings.push(...htmlReport.warnings);
  } catch (error) {
    report.warnings.push({
      code: "index_html_check_failed",
      message: "读取统一入口页失败，无法做文案与结构检查。",
      details: { error: String(error.message || error) }
    });
  }

  return report;
}

function printReport(report) {
  console.log(`客户端医生 @ ${report.generatedAt}`);
  console.log(`server: ${report.serverBase}`);
  console.log(`web:    ${report.webBase}`);

  console.log("\n检查项：");
  for (const check of safeArray(report.checks)) {
    const status = check.ok ? "OK " : "ERR";
    const extra = check.details?.status
      ? ` status=${check.details.status}`
      : check.details?.error
        ? ` error=${summarize(check.details.error, 80)}`
        : "";
    console.log(`- [${status}] ${check.kind} ${check.target}${extra}`);
  }

  if (report.autonomy) {
    console.log("\n自治指标：");
    console.log(`- 自治指数: ${report.autonomy.overallPercent}%`);
    console.log(`- 待复核任务: ${report.autonomy.counts.needsAttention}`);
    console.log(`- 裸 OPEN 任务: ${report.autonomy.counts.unmanagedOpen}`);
    for (const metric of safeArray(report.autonomy.metrics)) {
      console.log(`- ${metric.label}: ${metric.percent}% (${metric.description})`);
    }
  }

  console.log("\n失败：");
  if (!report.failures.length) {
    console.log("- 无");
  } else {
    for (const item of report.failures) {
      console.log(`- ${item.code}: ${item.message}`);
      if (item.title) {
        console.log(`  任务: ${item.title}`);
      }
    }
  }

  console.log("\n警告：");
  if (!report.warnings.length) {
    console.log("- 无");
  } else {
    for (const item of report.warnings) {
      console.log(`- ${item.code}: ${item.message}`);
      if (item.title) {
        console.log(`  任务: ${item.title}`);
      }
    }
  }
}

const isDirectRun = process.argv[1] && import.meta.url === new URL(`file://${process.argv[1].replace(/\\/g, "/")}`).href;

if (isDirectRun) {
  const argv = process.argv.slice(2);
  const json = hasFlag(argv, "--json");
  const failOnWarn = hasFlag(argv, "--fail-on-warn");
  const serverBase = optionValue(argv, "--server", "http://127.0.0.1:3000");
  const webBase = optionValue(argv, "--web", "http://127.0.0.1:1421");

  runDoctor({ serverBase, webBase })
    .then((report) => {
      if (json) {
        console.log(JSON.stringify(report, null, 2));
      } else {
        printReport(report);
      }

      if (report.failures.length > 0) {
        process.exitCode = 2;
      } else if (failOnWarn && report.warnings.length > 0) {
        process.exitCode = 1;
      }
    })
    .catch((error) => {
      console.error("客户端医生执行失败：", error);
      process.exitCode = 3;
    });
}

export {
  analyzeBoard,
  analyzeHtml,
  analyzeProjectSummaries,
  computeAutonomyMetrics,
  parseTaskVersion,
  runDoctor
};
