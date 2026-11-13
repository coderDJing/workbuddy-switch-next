#!/usr/bin/env node
/**
 * 上游漂移检查：把 `docs/upstreams.json` 里固定的 commit 与上游最新提交比对。
 *
 * 领积分这种事，协议随时会变（新任务、锁定规则、回执字段），所以不能只在
 * 文档里记一笔就算维护了。这个脚本给人工核对和每周的 CI 任务共用：
 *
 *   node scripts/check-upstreams.mjs              # 表格
 *   node scripts/check-upstreams.mjs --markdown   # GitHub Step Summary 用
 *   node scripts/check-upstreams.mjs --json       # 机器可读
 *   node scripts/check-upstreams.mjs --strict     # 有落后/未知就以 1 退出
 */
import fs from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const DEFAULT_MANIFEST = path.join(ROOT, "docs/upstreams.json");

/** CJK 字符占两列，表格才对得齐。 */
export function displayWidth(text) {
  let width = 0;
  for (const char of String(text)) {
    const code = char.codePointAt(0) ?? 0;
    width += code > 0x2e80 && code !== 0xfeff ? 2 : 1;
  }
  return width;
}

export function padDisplay(text, width) {
  const value = String(text ?? "");
  return value + " ".repeat(Math.max(0, width - displayWidth(value)));
}

/**
 * 单条上游的状态。
 * - `untracked`：没有固定 commit，只作参考，不参与漂移判断
 * - `behind`：上游在固定点之后有新提交
 * - `diverged`：上游历史被改写（behindBy > 0），需要人工确认
 * - `current`：与上游最新一致
 * - `unknown`：取不到（网络/权限/仓库改名）
 */
export function classify(entry, result) {
  if (!entry.pinned) return { state: "untracked", detail: "未固定" };
  if (!result || result.error) return { state: "unknown", detail: result?.error ?? "未检查" };
  if (result.behindBy > 0) {
    return { state: "diverged", detail: `历史已改写（本地 +${result.behindBy}）` };
  }
  if (result.aheadBy > 0) {
    return {
      state: "behind",
      detail: `落后 ${result.aheadBy} 个提交${result.latestDate ? `（最新 ${result.latestDate}）` : ""}`,
    };
  }
  return { state: "current", detail: "已是最新" };
}

const STATE_LABEL = {
  current: "最新",
  behind: "待跟进",
  diverged: "需人工确认",
  untracked: "仅参考",
  unknown: "取不到",
};

/** 渲染成可直接打印的表格；markdown 模式用于 Step Summary。 */
export function render(rows, { markdown = false } = {}) {
  if (markdown) {
    const lines = ["| 平台 | 上游仓库 | 锁定 | 状态 |", "| --- | --- | --- | --- |"];
    for (const row of rows) {
      lines.push(`| ${row.platform} | [${row.repo}](https://github.com/${row.repo}) | ${row.pin} | ${row.label}：${row.detail} |`);
    }
    return lines.join("\n");
  }
  const header = ["平台", "上游仓库", "锁定", "状态"];
  const body = rows.map((row) => [row.platform, row.repo, row.pin, `${row.label}：${row.detail}`]);
  const widths = header.map((cell, index) =>
    Math.max(displayWidth(cell), ...body.map((line) => displayWidth(line[index]))),
  );
  // 最后一列也补齐：列宽一致，测试与 grep 都更好断言（终端里看不出尾部空格）。
  const line = (cells) => cells.map((cell, index) => padDisplay(cell, widths[index])).join("  ");
  return [line(header), line(widths.map((width) => "-".repeat(width))), ...body.map(line)].join("\n");
}

function ghToken() {
  if (process.env.GITHUB_TOKEN) return process.env.GITHUB_TOKEN;
  try {
    return execFileSync("gh", ["auth", "token"], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
  } catch {
    return "";
  }
}

async function api(url, token) {
  const response = await fetch(url, {
    headers: {
      accept: "application/vnd.github+json",
      "user-agent": "workbuddy-switch-next-upstream-check",
      ...(token ? { authorization: `Bearer ${token}` } : {}),
    },
  });
  if (response.status === 404) throw new Error("仓库或提交不存在/无权限");
  if (response.status === 403 || response.status === 429) throw new Error("被限流，稍后重试");
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json();
}

/** 取「固定点之后上游多了多少提交」以及上游最新提交时间。 */
async function probe(entry, token) {
  if (!entry.pinned) return { skipped: true };
  try {
    const [compare, latest] = await Promise.all([
      api(`https://api.github.com/repos/${entry.repo}/compare/${entry.pinned}...HEAD`, token),
      api(`https://api.github.com/repos/${entry.repo}/commits?per_page=1`, token),
    ]);
    const head = Array.isArray(latest) ? latest[0] : null;
    return {
      aheadBy: Number(compare.ahead_by ?? 0),
      behindBy: Number(compare.behind_by ?? 0),
      latestSha: head?.sha?.slice(0, 7) ?? null,
      latestDate: head?.commit?.committer?.date?.slice(0, 10) ?? null,
    };
  } catch (error) {
    return { error: error instanceof Error ? error.message : String(error) };
  }
}

export async function collect(manifest, { token = "", concurrency = 4 } = {}) {
  const entries = manifest.upstreams ?? [];
  const results = new Map();
  let cursor = 0;
  const workers = Array.from({ length: Math.min(concurrency, entries.length || 1) }, async () => {
    while (cursor < entries.length) {
      const index = cursor++;
      results.set(entries[index].id, await probe(entries[index], token));
    }
  });
  await Promise.all(workers);
  return entries.map((entry) => {
    const result = results.get(entry.id) ?? {};
    const verdict = classify(entry, result);
    return {
      id: entry.id,
      scope: entry.scope ?? "claim",
      platform: entry.platform,
      repo: entry.repo,
      kind: entry.kind,
      pin: entry.pinnedLabel ?? (entry.pinned ? entry.pinned.slice(0, 7) : "-"),
      state: verdict.state,
      label: STATE_LABEL[verdict.state],
      detail: verdict.detail,
      latest: result.latestSha ?? null,
    };
  });
}

async function main() {
  const args = process.argv.slice(2);
  const manifestPath = args.includes("--file")
    ? path.resolve(args[args.indexOf("--file") + 1])
    : DEFAULT_MANIFEST;
  const markdown = args.includes("--markdown");
  const asJson = args.includes("--json");
  const strict = args.includes("--strict");

  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  const rows = await collect(manifest, { token: ghToken() });

  if (asJson) {
    console.log(JSON.stringify({ rows }, null, 2));
  } else {
    for (const scope of ["claim", "runtime"]) {
      const subset = rows.filter((row) => row.scope === scope);
      if (!subset.length) continue;
      if (!markdown) console.log(`\n${scope === "claim" ? "领取链路" : "运行时依赖"}\n`);
      console.log(render(subset, { markdown }));
    }
  }

  const pending = rows.filter((row) => row.state === "behind" || row.state === "diverged");
  const unknown = rows.filter((row) => row.state === "unknown");
  if (!markdown && !asJson) {
    if (pending.length) {
      console.log(`\n有 ${pending.length} 个上游在固定点之后有新提交：`);
      for (const row of pending) console.log(`- ${row.platform} → ${row.repo}（${row.detail}）`);
      console.log("按 docs/UPSTREAMS.md 的步骤评估是否需要跟进。");
    } else if (!unknown.length) {
      console.log("\n所有已固定的上游都与最新一致。");
    }
  }
  if (strict && (pending.length || unknown.length)) process.exitCode = 1;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  await main();
}
