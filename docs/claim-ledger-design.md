# 每日领取状态记忆（Claim Ledger）设计草案

> 目标：让每个「领取积分 / 领取奖励」按钮知道今天到底领没领完，避免用户反复触发长任务，
> 并能区分「已领完」「还剩几项」「今天没得领」。
>
> 状态：已按本稿落地（0.2.8 RC 周期）。实现偏差记在各节末尾的「实现」段。

## 1. 现状盘点（代码级）

| 入口 | 后端命令 | 一次触发实际做了什么 | 现在能知道什么 | 重复点会怎样 |
| --- | --- | --- | --- | --- |
| 账号页「领取积分」（WorkBuddy/CodeBuddy） | `run_daily_tasks(variant)` → `checkin::run_daily_all` | 该档位**全部账号** × 20+ 成长任务（桌面会话、专家、小程序、旅行、盲盒、工作台…） | 仅本次返回的 `done/total/rest/warnings`，**不落盘** | 整轮重跑，每账号几分钟 |
| 账号卡片「手动签到」 | `checkin(accountId)` | 单账号签到 | 官方状态接口 + 签到日志 | 已签到时菜单项已隐藏 |
| 设置页 / 托盘「一键签到」 | `checkin_all` | 全部账号签到 | 签到日志 + `all_accounts_checked_in_today()` | 幂等，但会重复发请求 |
| MiniMax「领取积分」 | `minimax checkin` | 每账号一次签到 | 服务端 `signin/status`（available/already/closed） | 每次先查状态再决定是否提交 |
| Trae「领取积分」 | `trae checkin` | 国内版每账号一次签到 | 服务端 `checkin_credits/status` | 已签到返回 already，但仍逐账号请求 |
| Qoder「领取积分」 | `claimQoderRewards` → daily-runner `qoder-*` | 活动领取 + Pro 升级 | 只有 `ok/message` 文案 | 重新跑 daily-runner |
| 豆包「领取奖励」 | doubao `checkin` + 后台 `cycle` | 每账号一次签到 | **后台已有 per-account/day 的 `Attempt{day,at,count,done}`**，页面按钮不读 | 页面按钮与后台记忆脱节 |

三个结论：

1. 唯一成体系的记忆是 `auto_checkin_logs.json`（WorkBuddy 签到，30 天 / 500 条）——它记的是**签到**，不是**每日任务**。
2. 豆包后台 `doubao/automation.rs` 已有「账号 × 日期 × done」的领取记忆（`Attempt`），这是现成的范式，但没有暴露给 UI。
3. WorkBuddy 的「领取积分」是最贵的入口，恰好也是**唯一没有任何记忆**的入口。

## 2. 要解决的三件事

1. **单一真相**：一份按「任务日 × 平台 × 账号（× 任务）」的记忆，手动、自动、后台周期共用。
2. **聚合派生**：从记忆算出按钮的三种形态——已领完 / 未领完（还剩 N）/ 未知。
3. **优先级与失效**：官方状态可查时以官方为准，查不到时以本地记忆为准，并明确标注核对时间。

## 3. 关键决策

### D1 「已领完」的口径 = 没有可领项，而不是全部已领（已定）

成长任务里天然存在长期锁定、未达标、服务端未下发的项，它们永远不可能变成 claimed。
若用 `done == total` 判定，按钮会永远显示「没领完」，比现状更烦。

因此每账号记录四个计数：

```
total     官方下发的任务总数
claimed   已领奖
claimable 可领未领（进度达标、未领）   ← 决定按钮态
blocked   锁定 / 未达标 / 未下发
```

状态映射：

| 条件 | state | 语义 |
| --- | --- | --- |
| `claimable > 0` | `partial` | 还剩 N 项可领，建议再点 |
| `claimable == 0 && total > 0` | `claimed` | 今日已领完 |
| `total == 0`（企业版 / 受限 / 未开放 / 无任务） | `blocked` | 今日无可领取，不算「没领完」 |
| 提交失败 | `error` | 可重试 |
| 结果未知（超时 / 回执丢失） | `unconfirmed` | 待确认，**不重复提交**（沿用豆包现有规则） |

实现提示：daily-runner 的 `_build_task_snapshot` 已经算出 `have/done/ready/locked`，
`_task_is_ready(row)` / `_task_is_done(row)` 也可直接复用。
只需在 `vendor/daily-runner/runner.py` 的 `workbuddy_cn` 返回里补
`claimable/blocked`（多一次 tasks GET 或复用终态行），**不必改上游脚本**。

### D2 粒度：三层，不混用

| 层 | 粒度 | 用途 | 保留 |
| --- | --- | --- | --- |
| L1 账号日态 | 任务日 × 平台 × 账号 × scope | 按钮聚合、卡片 chip | 30 天 |
| L2 任务明细 | 再 × 任务 code | tooltip / 明细面板 | 7 天 |
| L3 平台派生 | 平台 × 档位 | 不落盘，读时现算 | — |

`scope` 用于区分同一账号的两类行为：`checkin`（签到）、`daily`（成长任务）、`rewards`（Qoder/豆包奖励）。

### D3 真相优先级

```
官方可查询状态  >  本地记忆  >  未知
```

- **可查官方状态**（MiniMax / Trae / 豆包 / Qoder 活动行）：页面加载与操作后查询，结果回写记忆（`source: "server"`），按钮以服务端为准，天然不会「跨设备误判」。
- **只能本地记**（WorkBuddy 成长任务）：以记忆为准，UI 标注「上次核对 12:31」。
- 记忆**不设 TTL**，但 `day` 一变即自动失效；跨日不删记录，只不参与展示。

### D4 日期边界

统一用 **Asia/Shanghai 的「任务日」**：平台的重置点就是这个时区（Qoder 每日 10:00 UTC+8、
豆包与成长任务按北京时间切日），而 daily-runner 内部 `beijing_today()` 也已经是这个口径。

注意：`auto_checkin_logs.json` 现在用本地时区日期，本次**不动它**，
ledger 只做叠加，避免影响 `CreditStatsPage` 的既有统计。

### D5 「已领完」时按钮怎么表现（已定：点击弹确认）

按钮显示「今日已领完」（弱化样式 + BadgeCheck），**保持可点**；点击弹一次确认：

> 今日所有账号都已领完。
> 如果你在官方客户端或另一台设备领过，重新核对会再跑一遍每日任务（每账号约 1–3 分钟）。
> 　[取消]　[仍要重新核对]

理由：长任务动辄几分钟，不能被无意点开；但换设备领过、记忆过期时必须有逃生口。
确认弹窗只挡一次，不做二次确认。

补充：可查询官方状态的四个平台（MiniMax / Trae / 豆包 / Qoder）在页面加载时会校正记忆，
「已领完」几乎不会误判，确认弹窗主要服务于 WorkBuddy 每日任务。

### D6 存储

`~/.wb-switch/claim_ledger.json`：

```jsonc
{
  "version": 1,
  "entries": [
    {
      "day": "2026-11-12",
      "platform": "workbuddy",     // workbuddy | minimax | trae | qoder | doubao
      "variant": "cn",
      "accountId": "…",
      "scope": "daily",            // checkin | daily | rewards
      "state": "partial",
      "counts": { "total": 24, "claimed": 18, "claimable": 2, "blocked": 4 },
      "tasks": [ { "code": "chat_5", "state": "claimable" } ],
      "message": "成长任务完成 18/24",
      "updatedAt": 1762900000000,
      "source": "manual"           // manual | auto | server
    }
  ]
}
```

- 原子写、写锁、30 天裁剪（明细 7 天）。体量估算：30 账号 × 30 任务 × 7 天 ≈ 100 KB，可接受。
- 隐私：只存 accountId + 任务 code + 状态，**不存 token / 昵称 / 邮箱 / 原始响应**。

### D7 写入点

只在后端写入，前端只读：

- `checkin::checkin_account` / `run_daily_all` 结束时写 `scope=checkin|daily`
- `minimax::checkin` / `trae::checkin` / `doubao::checkin` / `qoder::claim` 结束时写 `scope=rewards`
- 服务端状态查询命中时写 `source=server`（用于校正本地记忆）
- 同一天同 key **覆盖更新**并累加 `attemptCount`，不追加新行

## 4. 接口与前端形态

Rust：`crates/wb-switch-core/src/modules/claim_ledger.rs`

```rust
pub fn record(platform, variant, account_id, scope, source, outcome);  // 覆盖更新 + attemptCount
pub fn snapshot() -> Value;                    // { day, timezone, entries }（只返回当天）
pub fn clear(platform, scope) -> io::Result<usize>;  // 清今天，返回删除条数
```

Tauri：`get_claim_ledger`、`clear_claim_ledger { platform?, scope? }`
HTTP（webui）：`GET /api/claim/ledger`、`POST /api/claim/ledger/clear`

`Entry.tasks` 存的是**中文任务名**（runner 用上游 `task_cn` 转好），前端直接展示，
不再各自维护 code → 名称映射。`state` 只会出现
`claimed / partial / blocked / error / unconfirmed / skipped` 六种。

前端：

- `src/lib/claim-status.ts`：纯函数 `deriveClaimView(platform, accounts, ledger, now)` → `{ mode, doneCount, totalCount, remainingLabels, updatedAt }`（可单测，参照 `newer-credit-result.ts` 的风格）
- `src/lib/use-claim-ledger.ts`：读 + 焦点/可见性刷新（复用 `useVisibleInterval`、`use-workbuddy-status-refresh`）

按钮四态：

| 态 | 文案 | 样式 | Tooltip |
| --- | --- | --- | --- |
| 未知 / 未领 | `领取积分` | 现状 outline | 现状 |
| 进行中 | `领取中…` + spinner | 现状 | — |
| 未领完 | `领取积分` + 角标 `剩 2` | outline | `3/5 个账号已领完；未完成：和AI聊天5次、夜猫子` |
| 已领完 | `今日已领完` | ghost / 弱化 + BadgeCheck | `5/5 个账号已领完 · 12:31 核对`（点击见 D5） |

账号卡片新增 chip（与现有「今日已签到」chip 并存、语义分离）：

- `已领完`（success 色） / `3 项待领`（secondary） / `无任务`（muted）
- tooltip 展开明细：`✓ 已完成 18 · ○ 可领 2 · · 未达标 4` + 具体任务名

**关键**：签到 chip 与任务 chip 必须分开。现在用户看到「今日已签到」会以为都领完了，
这正是「重复点」的心理来源。

实现：账号卡片用 `dailyClaimChip()`（`account-card.tsx`），其余四个平台用
`ClaimStateBadge`（`claim-action.tsx`），两者共用 `claimChipLabel` / `claimChipHint`。
chip 可点击，打开 `ClaimLedgerDialog`（`claim-ledger-dialog.tsx`）：逐账号列出
`已完成 / 可领 / 未达标` 计数、未完成任务名与其状态、原因、核对时间与触发来源，
今天还没跑过的账号也一并列出。

交互是「先看再领」：点「领取积分／领取奖励」不再直接跑任务，而是打开上面的弹窗；
弹窗底部才是真正的执行按钮（`领取全部`，已领完时叫 `重新核对`），并写明这次会影响
几个账号、大概要多久。跑完后的提示只报数量（`3 个账号已领完，2 个还有任务未完成`），
并带一个「查看详情」动作回到弹窗；不再把任务名、warning 或错误原文塞进 toast。

明细的展示深度（已定）：只做到 **chip + tooltip**，不做独立的「今日领取明细」面板。
tooltip 里给三档计数与最多 3 个任务名（超出的写「等 N 项」），需要完整信息时走「重新核对」看 toast 汇总。

## 5. 边界与坑

| 场景 | 处理 |
| --- | --- |
| 在官方客户端/另一台设备领过 | 记忆说已领完 → D5 的确认弹窗兜底；可查状态的平台由官方查询纠正 |
| `unconfirmed`（超时、回执丢失） | 标 `unconfirmed`，按钮显示「待确认」，不重复提交 |
| 页面跨 0 点 | 前端按 `day` 比对，跨日立刻回到未知；已有 1 分钟定时器可用 |
| 自动周期已领 | `auto` 写入后，页面下一次刷新即显示「已领完」，不需用户点 |
| 账号删除 / 换 id | ledger 按 accountId 保留，聚合只统计现存账号 |
| 企业版 / 受限 / 未开放 | 记 `blocked`，**不计入未领完**，否则永远提示 |
| 关闭自动签到的账号 | 记 `skipped`，不参与聚合，与现有 `excluded_account_ids` 语义一致 |
| demo / 截图模式 | ledger 走 `screenshot-demo.ts` 的假数据，不能读真实文件 |

## 6. 实施顺序（五平台一个版本内完成，按依赖分四步）

| 步 | 内容 | 可独立验证 |
| --- | --- | --- |
| S1 | `claim_ledger` 核心（结构 / 日期 / 裁剪 / 原子写 / 单测） | `cargo test -p wb-switch-core claim_ledger` |
| S2 | 写入侧接线：checkin / minimax / trae / doubao / qoder + runner 适配 | 跑一次领取后检查 `claim_ledger.json` |
| S3 | 读取侧：Tauri 命令 + `claim-status.ts` 纯函数 + 单测 | `npm test claim-status` |
| S4 | 五个页面按钮态 + 卡片 chip + 确认弹窗 + 设置页清除入口 | 手工回归五个平台 |

S2 内部按风险从高到低：WorkBuddy 每日任务（需 runner 补 `claimable`）→ Qoder（需 runner 补 `state`）
→ MiniMax / Trae / 豆包（纯映射已有 `state`）。

## 7. 改动面（文件级清单）

**后端**

- 新增 `crates/wb-switch-core/src/modules/claim_ledger.rs`；`modules/mod.rs` 注册
- `modules/checkin.rs`：`run_daily_all` 写 `scope=daily`，`checkin_account` 写 `scope=checkin`
- `modules/minimax/checkin.rs`、`modules/trae/checkin.rs`、`modules/doubao/checkin.rs`（或 `automation.rs`）：按返回 `state` 写 `scope=rewards`
- `modules/doubao/automation.rs`：`Attempt` 保持原样用于节流，成功/失败结果**镜像**写入 ledger（不迁移，避免动后台重试逻辑）
- `modules/qoder/credits.rs::claim`：消费 runner 新增的结构化 `state`
- `vendor/daily-runner/runner.py`：`workbuddy_cn` 返回补 `claimable/blocked`（复用 `_task_is_ready` / `_task_is_done`），`qoder` 返回补 `state`；`test_runner.py` 同步

**前端**

- 新增 `src/lib/claim-status.ts`（纯函数 + 单测）、`src/lib/use-claim-ledger.ts`
- `src/lib/api.ts`：`getClaimLedger` / `clearClaimLedger` + 类型
- `src/pages/AccountsPage.tsx` + `src/components/account-card.tsx`：按钮四态、任务 chip、确认弹窗
- `src/pages/MinimaxPage.tsx`、`src/pages/TraePage.tsx`、`src/pages/DoubaoPage.tsx`、`src/components/qoder-accounts-view.tsx`：按钮态 + 确认弹窗
- `src/pages/SettingsPage.tsx`：清除今日领取记录
- `src/lib/screenshot-demo.ts`：demo / 截图模式假数据

**Tauri**：`src-tauri/src/commands.rs` 两个命令 + `lib.rs` 注册

## 8. 风险

1. **runner 需要重新构建**：`prepare-daily-runner.mjs` 走 PyInstaller，补字段后必须重建 sidecar，
   CI 与本地都要跑一遍 `npm run prepare:daily-runner`（现有流程）。S1/S3 不依赖它，可先落。
2. **五平台同版回归面大**：建议 S4 按平台逐个自查，但对外只发一个版本。
3. **`claimable` 口径依赖上游任务表**：若某次任务表读取部分失败，`claimable` 可能偏高 → 显示「还剩 N」而不是「已领完」，
   属于安全侧偏差（宁可让用户点一次，也不误报已领完）。
4. **`blocked` 不能计入未领完**：企业版 / 受限 / 活动未开放必须单独归类，否则按钮永远红着。

