# WorkBuddy Switch Next

**五个平台，一个账号工作台。**

集中管理 **WorkBuddy / CodeBuddy、Qoder、TRAE、MiniMax Code、豆包** 的账号、额度与每日任务。桌面 App、浏览器 WebUI 和一次性 CLI 共用后端与账号数据，既适合日常切号，也便于 AI Agent 查询和管理。

<p align="center"><img src="public/icon-transparent.png" alt="WorkBuddy Switch Next" width="88" /></p>

[下载正式版](https://github.com/coderDJing/workbuddy-switch-next/releases/latest) · [下载 RC](https://github.com/coderDJing/workbuddy-switch-next/releases/tag/preview) · [在线演示](https://coderdjing.github.io/workbuddy-switch-next/) · [CLI 文档](docs/cli.md)

## 可以做什么

- **集中管理账号**：五个平台独立账号库，登录添加、本机识别、切换、重命名和备份迁移；按平台选择国内版或国际版。
- **查看官方额度**：余额、积分包、到期时间和查询快照按来源展示。豆包展示官方额度窗口百分比；缺失数据保留为未知。
- **安排每日任务**：按平台配置领取、查询、提醒与空闲轮换，查看执行记录；具体活动资格与可用区域以官方返回为准。
- **统一操作体验**：相同的账号卡片、响应式布局、排序与紧凑 / 宽松偏好，跨平台切换保留操作习惯。
- **交给 Agent 管理**：CLI 提供 JSON 输出、能力发现、稳定账号 ID、切换预演、状态与额度查询，以及命名操作入口。
- **延续工作现场**：WorkBuddy / CodeBuddy 提供会话复制、关联与增量同步；桌面版集成 Agent Companion 会话悬浮窗。

## 五个平台

| 平台 | 账号与客户端 | 额度与统计 | 每日任务与说明 |
| --- | --- | --- | --- |
| WorkBuddy / CodeBuddy | WorkBuddy、CodeBuddy IDE / CLI、VS Code 和 JetBrains 插件，共用账号库 | 官方积分、到期资源、请求与 Token 用量 | 每日任务、CodeBuddy CLI 轮换；支持会话管理 |
| Qoder | 国内版 / 国际版，桌面端与 CLI 凭据分别保存 | 官方积分、本地请求积分与 Token 统计 | 按区域领取、提醒和 CLI 空闲轮换 |
| TRAE | 国内版 / 国际版桌面端 | 官方额度、积分包与历史快照 | 国内版签到、查询与自动任务 |
| MiniMax Code | 国内版 / 国际版，桌面端与 CLI 共用登录态 | 官方积分、历史快照与本机合计 Token | 设备码登录、每日领取、查询、提醒与空闲轮换 |
| 豆包 | 桌面端，多 Profile 登录态整组保存 | 官方订阅额度窗口、百分比与历史快照 | 活动签到、查询、提醒与空闲轮换 |

各平台按实际能力提供页面。TRAE、豆包暂没有可靠 Token 统计来源；MiniMax Token 统计不能可靠按账号或区域归因。每日签到的活动资格、区域与设备限制分别判断。

详细说明：[五平台功能对照](docs/platform-parity.md) · [Qoder 登录验证](docs/research/qoder-first-login-feasibility.md) · [TRAE](docs/trae-integration.md) · [MiniMax Code](docs/minimax-integration.md) · [豆包](docs/doubao-integration.md)

## 安装与更新

仅支持 **Windows x64** 和 **macOS Apple Silicon（arm64）**。

| 系统 | 安装包 | 安装方式 |
| --- | --- | --- |
| Windows x64 | `workbuddy-switch-next-windows-x86_64-setup.exe` | 运行安装程序，按提示完成安装 |
| macOS Apple Silicon | `workbuddy-switch-next-macos-aarch64.dmg` | 将 App 拖入「应用程序」 |

从 [正式版 Releases](https://github.com/coderDJing/workbuddy-switch-next/releases/latest) 或 [RC 更新入口](https://github.com/coderDJing/workbuddy-switch-next/releases/tag/preview) 下载。版本与产物以对应 Release 为准，可用同页 `SHA256SUMS` 校验文件。

正式版通过 `latest` 更新，RC 通过 `preview` 更新，两个通道隔离。RC 转正式版需退出应用后手动安装正式版，保留 `~/.wb-switch` 数据目录即可。

macOS 产物尚未经过 Apple 公证。首次打开被系统拦截时，确认下载来自本仓库 Releases，再清除应用的隔离属性：

```bash
sudo xattr -rd com.apple.quarantine "/Applications/WorkBuddy Switch Next.app"
open "/Applications/WorkBuddy Switch Next.app"
```

切换时提示写入权限不足，可在系统设置「App 管理」或「完全磁盘访问」授权本应用，然后重启；WorkBuddy / CodeBuddy 平台设置提供权限检测。

## 开始使用

1. 在左侧选择平台，按需选择国内版 / 国际版。
2. 点击「登录添加」，或在官方客户端登录后选择「识别本机账号」。具体登录方式由平台决定。
3. 查看账号卡片上的额度、当前使用状态和到期信息；「刷新」查询官方数据，「领取」检查并执行活动。
4. 切换前保存正在进行的工作，按弹窗完成退出、写入与重新打开。CLI 默认遵循显式进程控制参数。
5. 在「平台设置」安排自动任务、查看执行记录和管理该平台备份；在全局设置调整外观、平台显示、WebDAV 连接与更新。

WorkBuddy / CodeBuddy 的每日任务可能耗时数分钟、创建对话或消耗积分与能量，请按需求设置账号任务开关。豆包签到只提交一次并回查，回执不明不会立即重复提交。

## 备份与切换

各平台独立保存账号，切换采用各自客户端所需的凭据结构、现场备份与结果校验。支持的操作及平台验证范围见各接入文档。

WebDAV 连接在全局设置配置，各平台管理独立备份对象。云端备份以密码加密，并用 ETag 防止并发覆盖；导入先预览，已有账号与冲突按平台规则处理。

豆包桌面快照绑定本机系统密钥，便携备份只迁移账号授权；导入后可查询额度和签到，新设备需登录并识别一次后才能切换。其他平台的迁移约束见对应文档。备份包含账号授权，请妥善保管文件和密码。

## CLI 与 WebUI

桌面安装包自带同版本命令。Windows 安装后、macOS 首次启动 App 后，请新开终端。也可使用 npm 分发包，见 [npm 说明](npm/README.md)。

```bash
workbuddy-switch-next                               # 启动本地服务并打开 WebUI
workbuddy-switch-next help --json                   # 查询能力清单
workbuddy-switch-next accounts --target qoder-cli
workbuddy-switch-next inspect --target minimax-desktop
workbuddy-switch-next credits --target doubao-desktop
workbuddy-switch-next switch <ACCOUNT_ID> --target trae-desktop --dry-run
```

默认输出 JSON，结果在 stdout，进度在 stderr；人工阅读可加 `--text`。先查询 `accounts` 获取目标平台账号 ID，再按 `help --json` 的能力清单选择命令。桌面端切换需要关闭并重开时，显式指定 `--close-running --relaunch`。

CLI 支持 `accounts`、`status`、`inspect`、`credits`、`switch` 等操作，统计、领取、登录和自动任务按目标能力开放。`operations` / `call` 可访问共用后端命名操作，完整输入与错误约定见 [CLI 文档](docs/cli.md)。持续自动任务需保持桌面应用或 `serve` 运行，Agent Companion 由桌面版管理。

## 界面预览

![多平台账号工作台](docs/images/next-workspace.png)

![平台设置与自动任务](docs/images/next-qoder-settings.png)

截图与 [在线演示](https://coderdjing.github.io/workbuddy-switch-next/) 使用虚构数据，演示禁用业务写入。截图可能早于最新接入的平台，在线版本以最近一次部署为准。

## 开发与验证

前端使用 React / TypeScript，桌面端使用 Tauri / Rust，共用 core 为桌面与 CLI 提供业务能力。

```bash
npm ci
npm run dev
npm run build
npm test
cargo check -p wb-switch-core -p wb-switch-server -p wb-switch-rust
```

开发、发布与维护说明：[开发说明](docs/DEVELOPMENT.md) · [维护说明](docs/FORK-MAINTENANCE.md) · [版本记录](docs/releases/)

Windows 豆包已验证本机识别、加密入库、官方额度与签到状态查询、预演和同账号切换核验；首次发奖、不同账号间真实切换、macOS 与真实 WebDAV 尚需对应环境验收。其他平台的实测边界记录在各自接入文档中。

## 迁移、致谢与许可

Next 是 [changexbc/workbuddy-switch](https://github.com/changexbc/workbuddy-switch) 的社区维护分支，使用独立应用标识，继续读取 `~/.wb-switch` 数据。迁移前正常退出旧应用；Next 更新配置独立保存，macOS 需按新应用名称授权。

感谢原作者与社区贡献者。每日任务参考 [WorkBuddy Daily](https://github.com/L0NE-6/WorkBuddy-Daily)、[Qoder Daily](https://github.com/L0NE-6/Qoder-Daily)；Qoder 协议参考 [NextAgentX/qoder-trae-workbuddy-switch](https://github.com/NextAgentX/qoder-trae-workbuddy-switch) 与 [JIUSHIQINGSHAN/qoder-switch](https://github.com/JIUSHIQINGSHAN/qoder-switch)；豆包协议参考 [TraeWorkAssistant](https://github.com/smart-open/TraeWorkAssistant)。其他协议参考与许可证见各平台接入文档。

桌面悬浮窗来自 [Agent Companion](https://github.com/changexbc/agent-companion)。感谢 [Linux.do](https://linux.do) 社区。

[MIT License](LICENSE)
