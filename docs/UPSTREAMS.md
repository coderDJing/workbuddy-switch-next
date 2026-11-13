# 上游清单与跟进

领积分这件事**协议随时会变**：新任务、锁定规则、回执字段、风控口径，任何一处变了，
我们自己写的判定就可能悄悄失效（表现为「明明还有东西可领，却显示已领完」）。
所以上游不能只记在脑子里——固定 commit、定期比对、按流程跟进。

机器可读的清单在 [`docs/upstreams.json`](upstreams.json)，本文解释怎么用它。

## 两类关系，不能混

| 关系 | 含义 | 出问题时的表现 |
| --- | --- | --- |
| `vendored` | 脚本整包拷进 `vendor/`，由我们直接执行 | 上游改了接口，脚本直接报错或静默少领 |
| `protocol` | 只对照它核对协议与语义，实现是我们自己写的 Rust | 上游换了字段/状态码，我们的判定会偏，但不报错 |

`protocol` 类最容易腐烂：代码没变、测试全绿，但语义已经过时。所以清单里每条都写了
`watch`——追上游时到底该看什么。

## 查漂移

```bash
npm run upstreams:check              # 表格，人工看
npm run upstreams:check -- --markdown  # GitHub Step Summary 用
npm run upstreams:check -- --json      # 机器可读
npm run upstreams:check -- --strict    # 有落后或取不到就以 1 退出
```

脚本用 GitHub API 把每条 `pinned` 与上游 `HEAD` 做 compare，输出「落后几个提交 / 上游最新日期」。
没固定 commit 的参考项标「仅参考」，不参与判断；取不到时标「取不到」，不会假装最新。

每周一早上 `.github/workflows/upstream-drift.yml` 会自动跑一次（也可手动 dispatch）。
它红了一般只有两种原因：某个上游有新提交，或者 API 被限流。

## 跟进流程

1. **看 diff 再动手**：`gh api repos/<repo>/compare/<pin>...HEAD --jq '.commits[].commit.message'`，
   先判断是「文档/其他平台」还是「我们用的那条链路」。
2. **只跟相关的**：`watch` 里列的点变了才需要动。比如 WorkBuddy-Daily 改的是别的平台脚本，
   就不该为了跟版本号而重新 vendored。
3. **vendored 的升级**：
   - 重新取快照覆盖 `vendor/daily-runner/upstream/<platform>`，保留 `LICENSE` 与 `NOTICE.md`；
   - **重新套用我们的集成补丁**（`vendor/daily-runner/README.md` 记着，目前 WorkBuddy 有 3 处，
     其中 `claimable/blocked` 那处直接决定「已领完」判定，漏了就会退化成按 `remaining` 猜）；
   - 跑 `cd vendor/daily-runner && python -m unittest test_runner` 与上游自带 tests。
4. **protocol 的升级**：只改我们自己的实现（`crates/wb-switch-core/src/modules/<platform>`），
   同步该平台 `docs/*-integration.md` 里的「参考实现」一节与 pin。
5. **改完必须真机验一次**：协议类改动只有真实领取能证明。至少覆盖
   「全部可领 → 已领完」「还有可领项」「锁定任务」「登录失效」四种状态。
6. 最后更新 `docs/upstreams.json` 的 `pinned` / `pinnedLabel`，跑一次
   `npm run upstreams:check` 确认变绿，再随下一个 RC 发出去。

## 许可

- `L0NE-6/WorkBuddy-Daily`、`L0NE-6/Qoder-Daily`、`L0NE-6/Trae-AutoCheckin`、
  `smart-open/TraeWorkAssistant`、`JeffTsuiCUG/dsh-connect-minimaxcode`、
  `MiniMax-AI/minimax-code`：MIT，可 vendored / 可参考。
- `jlcodes99/cockpit-tools`：**CC BY-NC-SA 4.0**，只能对照行为，不得复制代码，
  也不得用于商业分发版本。
- `cyz824/DoubaoSwitcher`、`VICqiufeeng/dsh-minimax-connect`：未声明许可，
  只当场景参考，一行代码都不要抄。

## 已知的坑

- `pinned` 必须是**完整 40 位 commit**，写分支名等于没固定（脚本会拒）。
- 上游改了任务 code，`task_cn` 映射会回退成裸 code（如 `skill_share`），
  详情弹窗里会直接显示英文 code——看到这种就先怀疑上游任务表变了。
- 「已领完」的口径依赖 runner 返回 `claimable`；如果 sidecar 没重建或补丁丢了，
  会退化成「有 remaining 就算未领完」，表现为按钮永远不显示已领完。
