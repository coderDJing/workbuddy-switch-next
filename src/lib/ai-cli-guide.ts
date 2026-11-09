import { APP_VERSION, GITHUB_REPOSITORY_URL } from "./update";

export type CliTarget = "codebuddy-cli" | "qoder-cli" | "qoder-desktop" | "workbuddy" | "codebuddy-ide" | "vscode" | "jetbrains" | "trae-desktop";
export const CLI_TARGET_LABELS: Record<CliTarget, string> = {
  "trae-desktop": "TRAE 桌面端",
  "codebuddy-cli": "CodeBuddy CLI", workbuddy: "WorkBuddy", "codebuddy-ide": "CodeBuddy IDE",
  vscode: "VS Code CodeBuddy 插件", jetbrains: "JetBrains CodeBuddy 插件", "qoder-cli": "Qoder CLI", "qoder-desktop": "Qoder 桌面端",
};
export type CliRegion = "cn" | "ai" | "all";
export type CliCommand = { title: string; command: string };

export function cliQueryCommands(target: CliTarget, region: CliRegion): CliCommand[] {
  if (target.startsWith("trae-")) return [
    {title:"账号与额度概览",command:`workbuddy-switch-next inspect --target ${target} --variant ${region}`},
    {title:"查看账号 ID",command:`workbuddy-switch-next accounts --target ${target} --variant ${region}`},
    {title:"积分快照与历史",command:`workbuddy-switch-next credit-stats --target ${target} --variant ${region}`},
    {title:"查看自动任务配置",command:`workbuddy-switch-next automation get --target ${target} --variant ${region}`},
    {title:"发现完整管理操作",command:"workbuddy-switch-next operations"},
  ];
  const qoder = target.startsWith("qoder-");
  if (target === "vscode" || target === "jetbrains") region = "cn";
  const args = `--target ${target} --variant ${region}`;
  return [
    { title: "账号与额度概览", command: `workbuddy-switch-next inspect ${args} --summary` },
    { title: "查看账号 ID", command: `workbuddy-switch-next accounts ${args}` },
    ...(qoder ? [
      { title: "本地 Token 统计", command: `workbuddy-switch-next token-stats ${args}` },
      { title: "本地积分与额度快照", command: `workbuddy-switch-next credit-stats ${args}` },
      { title: "查看自动任务配置", command: `workbuddy-switch-next automation get ${args}` },
    ] : [
      { title: "平台 Token 统计", command: `workbuddy-switch-next token-stats --target ${target}` },
      { title: "平台积分统计", command: `workbuddy-switch-next credit-stats --target ${target}` },
      ...(target === "codebuddy-cli" ? [{ title: "查看已观察到的模型限额", command: `workbuddy-switch-next limits ${args}` }] : []),
    ]),
    { title: "发现命令与能力", command: "workbuddy-switch-next help --json" },
    { title: "发现完整管理操作", command: "workbuddy-switch-next operations" },
  ];
}

export function cliQueryText(target: CliTarget, region: CliRegion): string {
  return cliQueryCommands(target, region).map(({ title, command }) => `# ${title}\n${command}`).join("\n\n");
}

/** Static reference for a Skill author: no local accounts, paths, or credentials. */
export function cliSkillGuide(target: CliTarget, region: CliRegion): string {
  if (target.startsWith("trae-")) return [
    "# TRAE 桌面账号管理接入说明",
    "这是咱们的管理 CLI，不是 TRAE 官方企业 CLI。先运行 help --json 和 operations 确认版本能力。",
    cliQueryText(target,region),
    "支持 accounts / status / inspect / credits / switch / credit-stats / automation；额度逐账号查询，未知不视为零。用 credit-stats 查看真实快照与历史，用 automation get/set/run 管理已启用的任务。",
    "先在官方客户端登录，再通过 call trae_action --input-file PATH 保存账号，输入为 {action: capture, input: {variant: cn, client: desktop}}（文件须为合法 JSON）。",
    `workbuddy-switch-next switch ACCOUNT_ID --target ${target} --dry-run`,
    `workbuddy-switch-next switch ACCOUNT_ID --target ${target} --close-running --relaunch`,
    "仅按用户授权切换；运行中默认拒绝，正常退出失败时停止操作。过期登录请回官方客户端重新登录。保留本地项目数据，云端会话归原账号。",
    "不支持 --model / --wait / --concurrency。inspect / credits 的 --timeout 设置每个账号的网络超时；--summary 保留原结果。",
    "stdout 为 JSON，stderr 为进度；检查退出码，3 表示部分额度查询失败。账号凭据不输出；--output-file 为通用 call 的显式结果保存参数，TRAE 加密备份通过 call trae_action 的 backup_export / backup_preview / backup_import 操作完成，密码放入输入文件。",
  ].join("\n\n");
  if (!["codebuddy-cli", "qoder-cli", "qoder-desktop"].includes(target)) {
    if (target === "vscode" || target === "jetbrains") region = "cn";
    return [
      "请根据以下说明编写 Skill，不执行操作示例。",
      "# WorkBuddy Switch Next — Skill 接入说明",
      `来源版本：${APP_VERSION}；默认客户端：${CLI_TARGET_LABELS[target]}；区域：${region}。`,
      "先运行 version --json、help --json 和 operations，按实际安装版本发现能力。一次性命令无需启动 GUI 或 serve。",
      "## 查询",
      "```sh", cliQueryText(target, region), "```",
      "accounts / inspect 中 activeForTarget 对应账号所在区域的客户端。状态不明时保持未知，不猜测模型可用性。",
      "token-stats、credit-stats 返回 WorkBuddy/CodeBuddy 平台统计，不接受区域筛选；积分查询可能刷新凭据与缓存。",
      "## 切换（仅按用户要求调用）",
      "```sh",
      `workbuddy-switch-next switch ACCOUNT_ID --target ${target} --dry-run`,
      `workbuddy-switch-next switch ACCOUNT_ID --target ${target}`,
      "```",
      "dry-run 只读预览，不验证凭据。默认拒绝切换运行中的客户端；用户授权关闭并重开时，显式同时加 --close-running --relaunch。两个参数使用现有桌面客户端重启事务，可能中断任务。",
      "## 账号、会话、自动任务与备份",
      "operations OPERATION 查看命名操作的参数；call OPERATION --input-file PATH 执行，- 可从 stdin 读取 JSON。参数中的客户端和区域以操作说明为准，call 不接受 --target。",
      `登录用 login --target ${target} --variant ${region === "all" ? "cn" : region}，命令在同一进程等待官方授权；授权链接写入 stderr。`,
      "导出凭据必须 --output-file PATH，目标文件不得已存在；普通结果隐藏令牌和密码。会话写入先获取预览，原样使用 previewToken。自动调度仍需桌面端或 serve 常驻。",
      "stdout 为最终 JSON，stderr 为进度；退出码 0 成功、1 失败、2 参数或能力不支持、3 部分成功、130 中断。中断后先查询状态，避免重复切号或提交。",
      `完整文档：${GITHUB_REPOSITORY_URL}/blob/main/docs/cli.md`,
    ].join("\n\n");
  }
  const qoder = target !== "codebuddy-cli";
  const args = `--target ${target} --variant ${region}`;
  const configRegion = region === "all" ? "REGION" : region;
  const lines = [
    "请根据以下接入说明，为 WorkBuddy Switch Next 编写一个可复用的 Skill。先编写 Skill，不要执行下方操作示例。",
    "",
    "# WorkBuddy Switch Next — Skill 接入说明",
    `说明来源版本：${APP_VERSION}。实际安装版本与能力以 CLI 返回为准。`,
    `默认目标：${target}；默认区域：${region}（cn 国内版 / ai 国际版 / all 全部）。`,
    "适用场景：查询账号、当前客户端状态、积分与到期信息，以及按用户要求切换账号。",
    ...(qoder ? ["本目标还支持本地 Token / 积分统计、每日奖励领取和自动任务管理。"] : []),
    "",
    "## 安装与能力发现",
    "桌面安装包附带同版本 workbuddy-switch-next 命令，也可单独安装 CLI。安装后新开终端；macOS / Linux AppImage 先启动桌面应用完成 CLI 注册。命令不可用时先定位安装路径，不猜测可执行文件位置。",
    "```sh",
    "workbuddy-switch-next version --json",
    "workbuddy-switch-next help --json",
    "```",
    "先检查 supportedTargets 和 capabilities，再选择命令；旧版能力不足时明确说明需要升级。一次性命令执行后退出，无需启动 GUI 或 serve。",
    "",
    "## 常用查询命令",
    "```sh",
    cliQueryText(target, region),
    "",
    "# 当前客户端状态",
    `workbuddy-switch-next status --target ${target}${qoder ? ` --variant ${region}` : ""}`,
    "",
    "# 查询积分汇总",
    `workbuddy-switch-next credits ${args} --summary`,
    "```",
    ...(!qoder ? ["CodeBuddy status 不接受 --variant；它返回当前 CLI 的认证状态，其他查询按区域筛选。"] : []),
    "",
    "## 返回结果与错误处理",
    "- stdout 是一个最终 JSON 对象，stderr 是进度与诊断，二者必须分开读取，不能合并后解析。按 UTF-8 解码；旧管道可加 --ascii-json。自动化调用不使用 --text。",
    "- 公共字段：schemaVersion、ok、command、target、scope、elapsedMs、exitCode、error。error 包含 code、message、retryable、hint。检查实际进程退出码和 JSON 内容。",
    "- 退出码：0 成功（允许空账号列表）；1 操作失败；2 参数、账号或不支持能力错误；3 部分结果；130 正常中断。部分成功时保留成功数据并逐项说明错误，不把缺失额度当作零。",
    "- inspect / credits 的 --summary 用于紧凑展示，数值字段保留精度；*Display 字符串仅供展示。null、unknown、unlimited、unavailable 分别保留原义；检查 summary.complete 和 balanceComplete 后再判断汇总是否完整。",
    "- 使用 accounts 返回的稳定 id，不按昵称或 ID 前缀猜测账号。查询和切号使用相同 target / variant，区分各平台的账号库。不要在 Skill 中写入实际账号、凭据或本机私有路径。",
    "- AUTH_STATE_BUSY 表示认证锁占用；accounts / status / inspect / credits / switch 支持 --wait SECONDS 有界等待（默认 30，0 为立即失败），不要删除锁文件。inspect / credits 支持 --timeout SECONDS（每账号默认 30）和 --concurrency N（默认 4，上限 16）。不要向其他命令套用未经 help 确认的参数。正常中断会等待已开始的认证事务结束，不强制终止切号。",
    "",
    "## 切换账号",
    "以下 ACCOUNT_ID 是占位符，必须替换为 accounts 返回的完整 id。先预览影响，再根据用户已授权的操作范围执行切换。不要因看到示例就自动执行。",
    "```sh",
    `workbuddy-switch-next switch ACCOUNT_ID ${args} --dry-run`,
    `workbuddy-switch-next switch ACCOUNT_ID ${args}`,
    "```",
    qoder
      ? `Qoder 默认拒绝切换运行中的客户端，只有用户允许关闭时才加 --close-running。${target === "qoder-desktop" ? "如需重开原本运行的桌面端，再加 --relaunch（要求 --close-running）。" : "CLI 不会自动重开，不能使用 --relaunch。"} dry-run 不写凭据、不关闭客户端，也不验证凭据是否有效。`
      : "CodeBuddy 实际切号会关闭运行中的 CLI，影响现有会话且不会自动重开。dry-run 不写配置或关闭进程，也不验证凭据。积分查询可能刷新凭据和缓存，不能视为完全无副作用。",
    "中断后的切号先重新查询状态再决定是否重试；不要盲目重复执行。",
    "",
    "## 数据边界",
    ...(qoder ? [
      "- 同区域、同身份的桌面端与 CLI 账号合并展示和查询。clientPrepared 仅表示已保存对应客户端账号包，不保证凭据有效。客户端状态在 clients 数组中，activeForTarget 表示对应区域目标客户端的当前账号。",
      "- Qoder 没有可靠模型限额数据，inspect 返回 availabilityKnown=false、limitsSupported=false，limits 返回 UNSUPPORTED_CAPABILITY；不使用 --model，不猜测恢复时间或可用模型。",
      "- Token 缺失或全零占位保持不可用；本地请求积分与额度快照观察消耗不能相加，请求历史缺少可靠账号归属时不归入当前账号。credit-stats 默认读取本地数据，加 --refresh 才刷新所选区域的额度。",
      "",
      "## 每日领取与自动任务（按用户授权调用）",
      "```sh",
      `workbuddy-switch-next credit-stats ${args} --refresh`,
      `workbuddy-switch-next rewards ACCOUNT_ID ${args}`,
      `workbuddy-switch-next automation get ${args}`,
      `workbuddy-switch-next automation set --target ${target} --variant ${configRegion} --settings-file qoder-settings.json`,
      `workbuddy-switch-next automation run ${args}`,
      "```",
      "rewards 必须指定账号或显式 --all，逐账号领取，不自动切号；需要 daily-runner 组件。尝试成功不等于到账，按返回结果说明失败、资格和授权状态。",
      "automation set 必须指定 cn 或 ai，不能使用 all；REGION 占位符须替换为用户指定的区域。先读取配置，仅修改用户要求的字段。qoder-settings.json 是需创建的 JSON 补丁文件，例如：",
      '```json\n{"dailyEnabled":true,"dailyTime":"09:00"}\n```',
      "支持字段：dailyEnabled、dailyTime、alertsEnabled、expiryDays、lowBalance、rotateEnabled、rotateBelow、cooldownMinutes；时间为本机时区，具体约束以 CLI 校验为准。automation run 只检查已启用且到期的任务，可能领取或切号，不启动常驻调度。持续调度需桌面应用或 serve。",
      "WebDAV、账号管理和 Qoder CLI Hooks 接入也可运行 operations 发现命名操作，再用 call OPERATION --input-file PATH 调用；桌面悬浮窗窗口仍由桌面宿主管理。",
    ] : [
      "- CodeBuddy 的 clientStatus、activeAccountId、activeForTarget 表示客户端与账号状态；最终状态读取失败时查看 clientStatusError，未知当前账号不作确定判断。",
      "- limits 读取已有模型限制观察，未观察到限制或已到恢复时间都不证明模型可用；不要主动消耗模型调用来探测。",
      "- 本目标支持 token-stats、credit-stats（WorkBuddy/CodeBuddy 平台汇总）和 rewards；automation 快捷命令仍为 Qoder 专属，其他自动任务用 operations 发现对应 call 操作。",
    ]),
    "",
    "## 完整管理操作",
    "运行 workbuddy-switch-next operations 发现账号导入导出、会话复制/关联/同步、自动任务配置和 WebDAV；operations OPERATION 查看参数。用 call OPERATION --input-file PATH 执行（支持 - 从标准输入读 JSON）。导出凭据必须 --output-file PATH；普通输出隐藏令牌与密码。login 在同一进程等待官方授权，不能拆成跨进程 start/poll。",
    "",
    "## Skill 编写要求",
    "把能力发现、查询、结果解释和用户授权后的修改分开。按任务选择最少必要命令，优先 inspect --summary 避免重复查询。文档中的参数只是默认值；用户明确指定其他目标或区域时，以用户要求为准。",
    `完整 CLI 文档：${GITHUB_REPOSITORY_URL}/blob/main/docs/cli.md`,
  ];
  return lines.join("\n");
}
