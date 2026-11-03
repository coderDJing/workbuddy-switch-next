# WorkBuddy Switch Next 维护说明

本仓库基于 [changexbc/workbuddy-switch](https://github.com/changexbc/workbuddy-switch)，在 [coderDJing/workbuddy-switch-next](https://github.com/coderDJing/workbuddy-switch-next) 继续维护。保留上游版权与 MIT 许可，修复仍可向上游提交。

## 项目名称

用户于 2026-11-01 选定 **WorkBuddy Switch Next**。项目介绍、应用侧栏、网页/桌面窗口标题及系统通知使用该名称；简介为“持续维护的 WorkBuddy 多账号管理工具，支持桌面 GUI 与 CLI”。保留原项目来源、版权与 MIT 许可。

当前仓库为 `coderDJing/workbuddy-switch-next`，首个独立维护正式版本为 `0.2.0`，完整变化见 [版本说明](releases/0.2.0.md)。此前的 `0.2.0-beta.1` 为历史预览版本；后续仅发布 RC 与正式版。安装名称、应用标识、npm 包名和更新签名已独立；账号库与认证锁继续使用原目录。

### 正式图标

2026-11-01 用户选定蓝色贝雷帽、白色 N 的原猫版本。设计母图为 [定稿 PNG](assets/branding/workbuddy-switch-next-icon-beret-v1.png)，生成提示词与批准状态见 [设计记录](assets/branding/workbuddy-switch-next-icon-beret-v1.json)。界面、favicon、桌面图标及托盘已经使用该版本；深浅色与小尺寸效果见 [尺寸检查](assets/branding/workbuddy-switch-next-icon-sizes.png)。

运行 `npm run icons:generate` 可从固定母图及透明衍生图重新导出全部图标。导出器复用本地 Tauri CLI，生成桌面 PNG/ICO/ICNS、仓库已有移动端资源，以及 32/36 px 原生托盘 RGBA；Windows 图标单独烘焙圆角。单色托盘以镂空保留表情与帽子上的 N。

## 产品维护范围

用户于 2026-11-01 明确要求按完整产品接续维护，实际同时使用 GUI 与 CLI。维护范围覆盖桌面 App、npm/webui、一次性 CLI，以及原产品支持的国内/国际版客户端接入；账号管理、认证与切换、积分和 Token 统计、会话复制/关联/同步、签到与旅行、悬浮窗及升级发布均纳入维护。

功能优先级按完整用户流程、影响程度和依赖排序。GUI 的置顶、排序、跨账号会话和悬浮窗等功能有独立价值；不再以 CLI 使用比例作为搁置 GUI 功能的依据。上游停更状态与维护者意图分开描述，本分支以可复现问题、代码审查和验证结果决定吸收内容。

## 维护基线

2026-11-01 建立 `codex/maintained-fork` 维护分支，基于上游 v0.1.62（`22b318c`），包含已有的 8 个增量提交，末端为 `f73451e`。`origin` 指向自己的仓库，`upstream` 指向原仓库。

| 范围 | 已有增量 |
| --- | --- |
| CLI 与并发 | 一次性账号、积分与限额查询和切换；共享认证与积分事务协调 |
| GUI 同步 | 感知外部 CLI 更新，拒绝迟到响应 |
| 认证恢复 | 认证恢复后清理残留的重新登录提示 |
| 凭据保护 | 防止旧扫码凭据回滚，保留刷新后 RT 到期信息；导出与令牌刷新串行化，保留已记录的刷新时间 |
| 开发验证 | CLI 文档与三平台测试流程 |

详细用法见 [CLI 文档](cli.md)，认证修复范围见 [认证恢复说明](auth-recovery.md)。上述内容是维护基线清单，不能替代测试结果或证明某个真实账号已恢复。

本轮修复统计测试的输入隔离：`WB_SWITCH_HOME` 不会重定向客户端日志目录，原测试会扫描本机历史。现在通过内部路径参数使用四组临时样本，并分别核对每个来源的 Token 数和记录数，避免测试耗时与个人日志体积相关。

## 上游 PR 整合（2026-11-01）

本轮审查的 18 个开放 PR 均已处理：14 个吸收或按当前架构改造，4 个由维护基线中的等价实现覆盖。没有将整批 PR 原样合并；存在依赖、旧快照及安全边界差异时，保留当前实现并补充测试。下表作者为所审查 head 的 Git 提交作者，版权与 MIT 许可继续保留。

| 来源 | 提交作者 / 审查时 head | 本分支结果 |
| --- | --- | --- |
| [#208](https://github.com/changexbc/workbuddy-switch/pull/208) | bao.x / `16a34d5` | 扩充 JetBrains 系列与 Android Studio 的目录、进程、显示名称及切换入口 |
| [#207](https://github.com/changexbc/workbuddy-switch/pull/207) | Nicholas / `0d663b1` | 复制/同步会话时同步置顶；仅处理本次选中且成功同步的会话，保留目标顺序及未知字段，写前备份 |
| [#206](https://github.com/changexbc/workbuddy-switch/pull/206) | Nicholas / `1a49478` | 保留原始 created_at；允许归档陈旧 active，但目标客户端仍运行时拒绝该操作 |
| [#203](https://github.com/changexbc/workbuddy-switch/pull/203) | Coder / `8f958a7` | 已有等价实现：导出认证保留已记录刷新时间 |
| [#202](https://github.com/changexbc/workbuddy-switch/pull/202) | Coder / `6645100` | 已由现有统一认证事务锁覆盖，避免切换写回刷新前令牌，不再引入第二套锁 |
| [#201](https://github.com/changexbc/workbuddy-switch/pull/201) | Luo-0712 / `b94b7a4` | 按账号 Token/调用统计与 GUI 筛选；归因冲突或身份不足保留为未归属，CLI 身份日志改为流式读取 |
| [#199](https://github.com/changexbc/workbuddy-switch/pull/199) | xshun978-svg / `536e428` | 空配置使用 08:00–22:00 分散签到窗口，每轮最多 8 个并错开请求；轮转游标防止认证失败账号挤占后续账号 |
| [#191](https://github.com/changexbc/workbuddy-switch/pull/191) | Coder / `9e9f361` | 维护基线已包含：认证恢复后清理重新登录提示 |
| [#186](https://github.com/changexbc/workbuddy-switch/pull/186) | Tenc Yao / `bec461d` | WebDAV 加密备份、选择恢复与双向同步，补齐下述并发、凭据新旧及失败恢复保护 |
| [#184](https://github.com/changexbc/workbuddy-switch/pull/184) | broadteam / `852144c` | 最近切换记录与多维排序；记账在认证锁内重读，CLI 复用已有事务锁 |
| [#181](https://github.com/changexbc/workbuddy-switch/pull/181) | Coder / `e516e5e` | 维护基线已包含：一次性 CLI 切号及账号、积分、限额查询 |
| [#162](https://github.com/changexbc/workbuddy-switch/pull/162) | Nicholas / `d533a7b` | 切换统计账号时保留请求用量 Tab |
| [#137](https://github.com/changexbc/workbuddy-switch/pull/137) | zhangjia / `e917774` | 保留较新的 Companion 0.2.2 / `14430ef` 基线，吸收过滤及兼容补丁机制，未回退到 PR 的旧快照 |
| [#121](https://github.com/changexbc/workbuddy-switch/pull/121) | dh0827 / `7887c7b` | 悬浮窗权限等待状态、会话生命周期、工程跳转与音效修复，并保留较新内部会话过滤 |
| [#120](https://github.com/changexbc/workbuddy-switch/pull/120) | dh0827 / `a7ee2d1` | 每分钟读取限额台账，失败保留已有结果；补请求序号与关闭/卸载失效处理 |
| [#119](https://github.com/changexbc/workbuddy-switch/pull/119) | dh0827 / `03da50c` | Windows 优雅关闭、UTF-8 输出与启动兜底；等待完整原进程集合退出，保留内核进程句柄避免 PID 复用误判 |
| [#118](https://github.com/changexbc/workbuddy-switch/pull/118) | dh0827 / `c4fa89c` | 补扫共享插件宿主日志；只接受日志中的认证 UID，不借用 CN IDE 当前账号 |
| [#29](https://github.com/changexbc/workbuddy-switch/pull/29) | zhangjia / `3029c55` | 按当前多目标架构重做 CN IDE 优雅关闭、SQLite 一致性备份、事务写入及解密回读验证；退出不明时不写库 |

### WebDAV 接纳边界

- 加密使用 AES-256-GCM 与 PBKDF2；新备份同时认证头部，限制可接受的 KDF 迭代范围，兼容已有格式读取。
- 首次上传使用 If-None-Match；覆盖已有备份必须取得强 ETag。同步先完成带条件的远端上传，再持认证锁核对本地原始内容；远端竞争或本地并发修改均不会被静默覆盖。
- 账号按 UID 与客户端类型匹配，保留本地账号 ID、创建时间、备注及最近使用信息。不同凭据只接受有明确较新刷新时间的版本，旧版本跳过，时间相同或未知的冲突报错。
- 恢复预览绑定加密备份 SHA-256，预览后云端变化必须重新读取。配置写入保留备份和回滚记录；后续失败时恢复此前配置，遇到外部并发修改则停止覆盖并报告备份位置。

### Companion 来源与补丁

保留 `14430ef9f3ec592f2c898fb088800cc7b7a70097` / 0.2.2 快照。`patches/agent-companion/host-compatibility.patch` 与版本清单记录维护差异。已实际从该固定提交归档、应用补丁并核对全部 292 个文件；不是仅检查补丁语法。更新脚本的回归测试覆盖幂等、忽略未提交源码以及失败保留原快照。

## 0.2.0 正式版准备（2026-11-03）

在上述维护基线上，0.2.0 同时包含以下 RC 阶段的变化：

- WorkBuddy / CodeBuddy 与 Qoder 平台导航分开，区分全局设置与平台设置；按客户端开关入口与状态轮询。
- Qoder 国内版 / 国际版桌面端与 CLI 独立账号管理，支持网页登录添加、本机识别、身份合并、备份导入导出，以及分类积分和可用到期信息展示。
- 桌面安装包内置同版本 CLI，补齐 Windows、macOS、Linux 的命令注册与安装包内容校验。
- 改善滚动条、macOS 标题栏和 Qoder 多客户端账号卡片，修复账号状态循环导致的内存增长。
- RC 使用上海时间版本号及 `preview` 更新入口；正式版使用 `latest`。普通诊断日志只在 RC 启用。

Qoder 的真实验证目前限于 Windows 国内版网页登录、已有登录文件的桌面切换与恢复，以及隔离目录下的官方 CLI 登录身份确认。全新桌面首次启动、国际版、macOS / Linux、自动续期与跨客户端同时使用仍待真实验证，详情见 [试验与验证记录](research/qoder-first-login-feasibility.md)。

## 后续维护

本轮完成上表 PR 的整合，不等于所有上游 Issue 已解决。后续仍需调查跨账号副本不可见（#200）、IDE 新增关联入口（#195）、批量归档选择（#165）、企业登录（#177）及旧 WebKit（#196）。工作记忆、专家团、自动化跨账号迁移（#209、#154、#150）需另行确认数据所有权及恢复路径。

维护范围继续覆盖完整 GUI 与 CLI、签到/旅行、悬浮窗和平台发布。定期检查上游差异，按具体问题保留复现、实现与验证；不因 CLI 使用比例而搁置 GUI 功能。账号库、认证文件、扫码候选、凭据日志和签名私钥不得提交 Git。

## 验证

2026-11-01 Windows 本机结果：

- 主前端 108 项测试、生产构建通过；演示界面检查了排序、统计 Tab 保留、按账号 Token 筛选及切换来源重置、WebDAV 设置卡片。
- 核心完整单测 850 项及集成测试通过；随后新增的 Windows 进程句柄身份单测单独通过。WebDAV 端到端测试使用本地模拟服务及三个隔离设备目录，覆盖加密、合并、竞争和恢复预览失效。
- CLI/server 的 35 项单测、21 项端到端测试及 2 项摘要测试通过；npm launcher 5 项通过、1 项仅 Unix 测试在 Windows 跳过。
- Tauri 桌面 34 项测试通过；主程序 core/server/Tauri 的 lib、bins、tests 严格 Clippy 通过。
- Companion 前端 177 项通过、4 项跳过；类型检查、lint、构建及 bundle 检查通过。Rust workspace 146 项测试通过，Rust/Node 状态回放一致性检查通过。
- `npm run build:desktop` 通过，包含优化构建的原生 Companion runtime；该真实可执行文件通过隔离目录下的 hooks、异步等待/回答、双客户端、服务锁、所有权交接及认证 RPC 检查。
- Companion 固定基线补丁重放及文件逐一核验通过；三平台 CI 已补充 Companion 前后端检查。

上述账号、认证、会话和 WebDAV 测试使用合成数据或隔离目录，未切换本机实际保存账号，也未连接用户真实 WebDAV 服务。macOS/Linux 及真实客户端切换尚未在本机验证，配置 CI 不代表远端已通过。

常用命令：`npm test`、`npm run build`、`cargo test -p wb-switch-core`、`cargo test -p wb-switch-server`、`cargo test -p wb-switch-rust`、`node --test npm/tests/cli-launcher.test.cjs`、`npm run build:desktop`。Companion 验证命令及集成说明见 [集成文档](agent-companion-integration.md)。

## 发布状态

### 0.2.0 本地准备验证（2026-11-03）

- Windows 本机：正式版身份与版本校验、Cargo.lock 三个项目包版本及文档本地链接通过。
- 主前端 121 项测试、生产构建通过；发布与 npm launcher 测试 19 项通过、1 项 Unix 专属测试跳过。
- core 910 项单测通过、5 项按既有规则忽略，核心集成测试通过；server 36 项单测与 23 项端到端测试、桌面 33 项单测通过。三个包的格式检查及严格 Clippy 通过。
- 更新地址测试已按包版本分别校验正式版 / RC 的预期入口。README 主界面截图使用当前只读演示重新采集，并新增 Qoder 页面预览。
- 上述结果为本地验证；三平台 CI、四平台安装包构建、签名及 Release 发布以对应版本的 GitHub Actions 和 Release 实际产物为准。

`0.2.0` 使用正式版通道。四平台构建、更新包签名、完整更新清单及校验和由 `.github/workflows/build.yml` 产出。手动运行仅上传构建 artifacts；推送匹配版本的 tag 后才发布版本 Release。正式版标记为 GitHub latest；RC 标记为预发布并更新 `preview` 入口，成功后清理旧 RC 的 Release、tag 及过期 preview 附件，保留当前 RC、正式版及其他历史预发布类型。

Next 使用独立签名和应用标识，正式版与 RC 分别从自有仓库的 `latest` / `preview` Release 获取更新，清单内的下载 URL 固定到版本 tag。两通道不互相更新；RC 用户转正式版需手动安装。更新源配置写入 `github_config_next.json`；旧配置仅作一次性读取与代理迁移，不修改原配置或账号文件。

Windows 本机验证、三平台 CI 与正式发布结果按当前执行回执更新；配置完成不能替代这些运行结果。发布操作和签名维护详见 [开发指南](DEVELOPMENT.md)。

早期预览版 Windows 试装通过，实际安装位置为 `D:\Program Files\WorkBuddy Switch Next`。当时的签名安装包、独立 CLI、版本与更新来源已在隔离环境核对，账号库无实际切号操作。验证回执位于本机 `target/next-smoke-*`，不替代本次正式版的安装包验收。
