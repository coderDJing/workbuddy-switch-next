# WorkBuddy Switch Next

基于 [WorkBuddy Switch](https://github.com/changexbc/workbuddy-switch) 的社区维护版本，提供桌面 GUI、浏览器 webui 和一次性 CLI。支持 WorkBuddy / CodeBuddy 各端、JetBrains 插件与 Qoder 国内版 / 国际版的账号管理与切换。

从 [正式版 Releases](https://github.com/coderDJing/workbuddy-switch-next/releases/latest) 获取桌面安装包或独立 CLI；桌面安装包已自带同版本命令。完整功能、截图和平台限制见 [项目 README](https://github.com/coderDJing/workbuddy-switch-next#readme)。

## 安装与启动

```bash
workbuddy-switch-next              # 启动本地服务 + 自动打开浏览器
workbuddy-switch-next version      # 查看版本
```

支持 Windows x64、macOS Apple Silicon（arm64）。桌面安装后请新开终端；macOS 需先启动桌面应用完成命令注册。独立 CLI 可按完整路径运行。

Next 继续使用 `~/.wb-switch` 账号库，无需删除或迁移账号数据；使用前退出不支持共享认证锁的旧版 GUI。

## 功能

| 模块 | 说明 |
| --- | --- |
| WorkBuddy / CodeBuddy | 共用账号库；支持 WorkBuddy、CodeBuddy IDE、CLI、VS Code 与 JetBrains 插件账号切换，按客户端开关入口 |
| Qoder | 独立账号库；支持国内版 / 国际版桌面端与 CLI 的网页登录添加、本机识别、积分查询、切换和备份迁移 |
| 会话管理 | WorkBuddy / CodeBuddy 支持会话复制、跨账号关联、增量同步及分歧处理，具体能力按客户端区分 |
| 用量统计 | WorkBuddy / CodeBuddy 的积分到期、请求用量、Token 趋势及按账号 / 模型统计 |
| 平台设置 | CodeBuddy CLI 自动轮换、自动签到、限额监听及权限检测 |
| WebDAV | 加密备份、选择恢复及跨设备同步，包含凭据新旧判断与并发保护；不包含 Qoder 账号 |

webui 不提供桌面专属的 Agent Companion 悬浮窗。Qoder 目前完成 Windows 国内版网页登录、已有登录文件的桌面切换与恢复，以及隔离目录下的 CLI 登录身份确认；全新桌面首次启动、国际版、macOS、自动续期和跨客户端同时使用尚未完成真实验证。各端的功能矩阵与限制见 [项目 README](https://github.com/coderDJing/workbuddy-switch-next#支持的工具)。

切换 CodeBuddy CLI 会关闭运行中的 CLI，不自动重开。设置 →「支持工具」控制各端入口；自动轮换、签到、限额监听和权限检测位于 WorkBuddy / CodeBuddy →「平台设置」。

## CLI / AI 调用

无需打开 GUI，一次性命令执行后退出：

```bash
workbuddy-switch-next inspect --summary              # 账号、积分、到期和模型限额
workbuddy-switch-next accounts                       # 账号 ID 列表
workbuddy-switch-next status --target codebuddy-cli  # 当前 CLI 账号
workbuddy-switch-next credits --summary              # 积分汇总
workbuddy-switch-next limits --model hy3             # 模型限额与恢复时间
workbuddy-switch-next switch <ACCOUNT_ID> --dry-run  # 切号预览
workbuddy-switch-next switch <ACCOUNT_ID>            # 实际切号
```

一次性账号切换命令目前只支持 `codebuddy-cli`，Qoder 等其他客户端通过 GUI / webui 操作。查询与切换默认 JSON，人工阅读加 `--text`；结果在 stdout，进度在 stderr。积分查询可能刷新凭据并写入缓存；未观察到限额不代表模型可用。

完整参数、返回约定及 PATH 排查见 [CLI 文档](https://github.com/coderDJing/workbuddy-switch-next/blob/main/docs/cli.md)，或运行 `workbuddy-switch-next --help`。

## 升级与权限

桌面版使用 Next 签名更新源：正式版走 `latest`，RC 走 `preview`，不互相自动迁移；RC 转正式版请手动安装正式版。独立 CLI 可从 Release 下载新版替换。

macOS webui 的文件权限由启动服务的终端决定，切换账号遇到权限错误时按提示授权终端。桌面安装与 macOS 授权步骤见 [项目 README](https://github.com/coderDJing/workbuddy-switch-next#macos-权限说明)。

## 许可

[MIT](https://github.com/coderDJing/workbuddy-switch-next/blob/main/LICENSE)。保留原项目版权与来源。
