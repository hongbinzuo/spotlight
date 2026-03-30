import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";

const indexHtml = fs.readFileSync(
  path.resolve(import.meta.dirname, "../index.html"),
  "utf8"
);

test("桌面壳文案不再使用静态版本号作为主标题", () => {
  assert.equal(indexHtml.includes("Spotlight 0.1.0"), false);
  assert.equal(indexHtml.includes("<title>Spotlight 自举客户端</title>"), true);
  assert.equal(indexHtml.includes("<h1>Spotlight 自举客户端</h1>"), true);
});

test("桌面壳明确保留统一入口、项目入口和移动端路径", () => {
  assert.equal(indexHtml.includes("任务为中心的桌面入口"), true);
  assert.equal(indexHtml.includes("项目入口"), true);
  assert.equal(indexHtml.includes("服务可用后，这里会优先展示最需要推进、恢复或复核的项目"), true);
  assert.equal(indexHtml.includes("左侧任务列表"), true);
  assert.equal(indexHtml.includes("服务可用后，这里会优先展示当前项目里最需要推进的任务"), true);
  assert.equal(indexHtml.includes("焦点任务活动"), true);
  assert.equal(indexHtml.includes("搜索详细日志"), true);
  assert.equal(indexHtml.includes("统一工作区"), true);
  assert.equal(indexHtml.includes("移动端与浏览器仍然继续走统一入口"), true);
});
