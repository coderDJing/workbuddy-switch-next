import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import assert from "node:assert/strict";

import { classify, displayWidth, padDisplay, render } from "./check-upstreams.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const pinned = { pinned: "abc123" };

test("分开报告最新、落后与历史被改写", () => {
  assert.equal(classify(pinned, { aheadBy: 0, behindBy: 0 }).state, "current");
  assert.equal(classify(pinned, { aheadBy: 3, behindBy: 0 }).state, "behind");
  assert.equal(classify(pinned, { aheadBy: 3, behindBy: 2 }).state, "diverged");
});

test("没有固定 commit 的参考项不参与漂移判断", () => {
  assert.equal(classify({ pinned: null }, { aheadBy: 9 }).state, "untracked");
});

test("取不到时标记 unknown，而不是假装最新", () => {
  const verdict = classify(pinned, { error: "被限流，稍后重试" });
  assert.equal(verdict.state, "unknown");
  assert.match(verdict.detail, /限流/);
});

test("落后时带上上游最新提交日期", () => {
  const verdict = classify(pinned, { aheadBy: 1, behindBy: 0, latestDate: "2026-11-12" });
  assert.match(verdict.detail, /2026-11-12/);
});

test("中文列宽按两列计算，表格对齐", () => {
  const rows = [
    { platform: "豆包", repo: "smart-open/TraeWorkAssistant", pin: "b075a81", label: "待跟进", detail: "落后 2 个提交" },
    { platform: "WorkBuddy / CodeBuddy", repo: "L0NE-6/WorkBuddy-Daily", pin: "v3.41", label: "最新", detail: "已是最新" },
  ];
  const lines = render(rows).split("\n");
  assert.equal(lines.length, 4);
  assert.equal(new Set(lines.slice(2).map((line) => displayWidth(line))).size, 1);
  assert.equal(displayWidth("豆包"), 4);
  assert.equal(padDisplay("豆包", 6), "豆包  ");
});

test("markdown 模式给仓库加链接，供 Step Summary 使用", () => {
  const text = render(
    [{ platform: "TRAE", repo: "L0NE-6/Trae-AutoCheckin", pin: "d289f0d", label: "最新", detail: "已是最新" }],
    { markdown: true },
  );
  assert.match(text, /\| --- \|/);
  assert.match(text, /\[L0NE-6\/Trae-AutoCheckin\]\(https:\/\/github\.com\/L0NE-6\/Trae-AutoCheckin\)/);
});

test("清单里的 pin 必须是完整 commit，不能写分支名", () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(ROOT, "docs/upstreams.json"), "utf8"));
  for (const entry of manifest.upstreams) {
    if (entry.pinned === null) continue;
    assert.match(entry.pinned, /^[0-9a-f]{40}$/, entry.id);
  }
});

test("清单记录了位置与要跟进的内容", () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(ROOT, "docs/upstreams.json"), "utf8"));
  for (const entry of manifest.upstreams) {
    assert.ok(entry.id, "id");
    assert.match(entry.repo, /^[\w.-]+\/[\w.-]+$/, entry.id);
    assert.ok(entry.doc, `${entry.id} 需要指向记录文档`);
    assert.ok(entry.watch?.length, `${entry.id} 需要写明追上游时看什么`);
    if (entry.kind === "vendored") assert.ok(entry.localPath, `${entry.id} 需要记录快照位置`);
  }
});

test("五个领取平台都有上游记录", () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(ROOT, "docs/upstreams.json"), "utf8"));
  const platforms = manifest.upstreams.filter((entry) => entry.scope === "claim").map((entry) => entry.platform);
  for (const name of ["WorkBuddy", "Qoder", "TRAE", "豆包", "MiniMax"]) {
    assert.ok(platforms.some((platform) => platform.includes(name)), `缺少 ${name} 的上游记录`);
  }
  const ids = manifest.upstreams.map((entry) => entry.id);
  assert.equal(new Set(ids).size, ids.length, "id 不能重复");
});
