# 开发与发布指南

Next 的发布身份与版本入口是 `release-config.json`。账号库与认证锁继续使用 `~/.wb-switch`，应用安装身份、更新配置与签名属于 Next。

## 环境与构建

Node.js 22、Rust stable（至少 1.89）及对应平台的 Tauri 构建依赖。桌面构建还需要 Python 3.9+ 和 venv：构建脚本在 `target/` 下安装固定版本的 requests 与 PyInstaller，打包每日任务组件，应用运行时不需要另行安装 Python。构建环境需要匹配目标架构。

```bash
npm ci
npm run release:check
npm test
npm run build
cargo test -p wb-switch-core
cargo test -p wb-switch-server
```

桌面构建包含固定版本 Companion 前端、原生 runtime 与每日任务组件：

```bash
npm run build:desktop
# Windows 签名 NSIS 安装包
node scripts/build-release.mjs --target x86_64-pc-windows-msvc --bundles nsis
# macOS .app
npm run build:app:release
```

`build-release.mjs` 读取已配置的 TAURI_SIGNING_PRIVATE_KEY / TAURI_SIGNING_PRIVATE_KEY_PASSWORD，或本机 `~/.wb-switch-next/release-signing/updater.key` 与 `password.txt`。只加载到构建子进程，不输出密钥内容。

## 版本与分发身份

运行 `npm run version:set -- rc` 生成上海时间的 `X.Y.Z-rc.YYYYMMDDHHmm`，或用 `npm run version:set -- stable` 去掉 RC 后缀。脚本同步前端、三个 Rust 包、Tauri、npm 主包和平台包；再运行 Cargo 命令更新 Cargo.lock。仅允许 RC 与正式版，Beta / Alpha 会被版本脚本、发布校验和更新清单生成器拒绝。RC 使用 preview 更新入口；正式版本使用 Releases latest 入口，两个通道不互相更新。

- 应用名称：WorkBuddy Switch Next。
- 应用标识：`io.github.coderdjing.workbuddyswitchnext`。
- npm 主包：`workbuddy-switch-next`。
- npm 平台包：`workbuddy-switch-next-<platform>-<arch>`。
- 主命令：`workbuddy-switch-next`；保留 `workbuddy-switch` 别名，JSON 输出协议及认证锁兼容。
- 原生 CLI 内部二进制仍为 `wb-switch`；它与 Cargo 包名属于内部构建标识。

## RC 诊断日志

版本的首个预发布标识为 `rc` 时启用诊断（例如 `0.2.0-rc.202611031733`），正式版不写入普通诊断，正常错误日志继续保留。日志为 `~/.wb-switch/diagnostics/rc-debug.log`，每份最多 1 MiB，保留一份 `.1` 备份。记录启动、IPC 接收/分发与更新检查各阶段，只传递命令名、状态、计数和耗时；禁止记录命令参数、凭据、响应正文、代理地址或原始错误字符串。桌面设置的自动更新区可打开日志位置。

## 正式版发布准备

1. 对齐最新 `main`，核对本轮功能、修复和已知限制；同步根 README、npm README、维护说明与 CLI 文档。
2. 运行 `npm run version:set -- stable`，将当前 RC 的基础版本转为正式版；用 `cargo check -p wb-switch-core` 更新 Cargo.lock。
3. 新建 `docs/releases/<版本>.md`。发布工作流直接读取此文件，必须写明本版变化、安装迁移、更新通道和验证边界。
4. 运行 `npm run release:check`、前端测试与生产构建、发布脚本测试、Rust 测试和格式检查。核对 Cargo.lock 中三个项目包的版本，以及 Tauri 和 core 的正式版更新地址。
5. 检查通过后，按「提交 → push main → 打对应版本 tag → push tag」执行。准备阶段可以停在本地；只有推送 tag 才触发正式发布。两个支持平台构建与签名成功后，才以 Release 的实际产物作为发布完成依据。

从正式版开始新 RC 周期时，用 `npm run version:set -- rc`，脚本将补丁位加一并按十进位进位，例如 `1.1.9 → 1.2.0`；已有 RC 保持基础版本不变。RC 用户转正式版需手动安装；正式版发布不改写 `preview` 更新入口。

## 上游跟进

领取链路依赖的上游（脚本快照 + 协议参考实现）固定在 [`docs/upstreams.json`](upstreams.json)，
跟进流程见 [UPSTREAMS.md](UPSTREAMS.md)。日常只需：

```bash
npm run upstreams:check          # 看有没有上游在我们固定点之后改了东西
```

每周一 `.github/workflows/upstream-drift.yml` 会自动跑 `--strict`；它标红意味着该去评估上游变更，
不是代码坏了。

## 依赖补丁（vendor-patched）

`Cargo.toml` 的 `[patch.crates-io]` 指向仓库内的改写版本，改动都只修一个具体问题：

| 包 | 改动 |
| --- | --- |
| `indexmap` | 维持既有补丁，用于本地缓存键的哈希行为。 |
| `tauri-build` 2.6.3 | `copy_binaries` 删除已存在的 sidecar 时不再 `unwrap`：hosted macOS runner 上该 `remove_file` 会返回 EPERM，直接中断整次构建；紧随其后的 `fs::copy` 本身会覆盖目标，删除失败可以忽略。 |

升级 `tauri-build` 时需同步更新 `vendor-patched/tauri-build`，并确认上游是否已修复该处 `unwrap`。

## 更新签名

Next 使用自己的 Tauri updater 签名。当前机器的签名材料在用户目录 `~/.wb-switch-next/release-signing/`，公钥写入 `src-tauri/tauri.conf.json`。私钥与密码不得进入仓库，应单独备份；后续版本必须使用相同密钥以便已有安装验证更新。

新维护环境可用 `npm run tauri -- signer generate -w <私钥路径>` 生成新密钥，但已有用户安装后不能随意换钥。CI 使用仓库 Secrets：

- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

这属于更新包签名，不等于 Windows Authenticode 或 Apple 公证。系统开发者签名可另外配置。

## GitHub Actions

`test.yml` 在 push / PR 上运行三平台测试与前端检查。`build.yml`：

1. 手动触发可选择 Windows、macOS ARM/Intel、Linux 或全部平台，仅上传构建 artifacts。
2. 先复用完整三平台 test 工作流作为发布门禁，再检查发布身份与版本；构建后收集 CLI、安装包、签名和平台更新清单。
3. 按「提交 → push main → 打 tag → push tag」发布。版本 tag 必须等于所有包的版本，例如 `v0.2.0-rc.202611020100`。tag 构建四个平台，任一构建失败均不发布 Release。
4. 合并清单必须覆盖四种目标与 Linux 两种安装形态；所有清单的版本、下载来源、归档及签名必须一致。
5. 生成 SHA256SUMS，再发布版本 Release。正式版设为 GitHub latest；RC 另外更新名为 preview 的发布入口，成功后清理旧 RC Release、tag 和 preview 中过期的附件，保留当前 RC、正式版及其他历史预发布类型。

预览清单位于 `/releases/download/preview/latest.json`；清单里的实际安装包 URL 指向不可变版本 tag，避免拉取更新清单后入口移动导致版本错配。预览版不回退到 GitHub `/latest`，因为它不包含预发布版本。

macOS DMG 由固定版本 dmgbuild 制作，支持无 Finder 的 CI。Windows 安装包按实际 productName 与版本定位；清理旧产物通过 Node 原生文件操作限定在当前 target 的 bundle 目录。

## 桌面分发

CLI 与每日任务组件由桌面安装包内置，发布流程只上传桌面安装包、自动更新包、签名、更新清单与校验和。Windows 和 macOS 的打包校验会检查内置组件。

发布说明由 `release-artifacts.mjs notes` 添加顶部两个安装包直达链接，版本 Release 与 preview 共用不可变版本 tag 的下载地址。更新清单与兼容地址继续保留。

## 数据与配置兼容

Next 使用 `~/.wb-switch`，不复制或重建账号库。独立安装标识允许系统识别 Next；运行前退出不支持认证事务锁的旧版。

Next 更新配置为 `github_config_next.json`。首次读取原 `github_config.json` 时迁移原上游/旧 fork 地址并保留代理，只写新配置。自定义更新源保留，密钥验证仍使用 Next 公钥。恢复 WebDAV 配置也通过相同规范化入口。

测试和试装必须使用隔离的 WB_SWITCH_HOME；不调用真实切号来验证安装包。需要测试真实账号时另行记录客户端运行状态与授权范围。

## 图标

`npm run icons:generate` 从已批准的贝雷帽母图导出网页、桌面、已有移动端及托盘资源。Windows 圆角、单色托盘的表情与 N 镂空由导出脚本处理，设计来源与尺寸检查见维护说明。

## 本地构建缓存清理

`npm run build` 和桌面打包成功后自动检查本项目 `target` 中的 Rust 中间缓存。增量缓存超过 8 GiB，或中间缓存合计超过 30 GiB 时，先清理增量缓存；其余缓存仍超过 30 GiB 时再清理依赖、构建脚本输出与编译指纹。编译进程运行中或无法检查进程状态时跳过，自动清理失败不改变构建成功结果。

- `npm run clean:build-cache:check`：预览可清理的路径与大小。
- `npm run clean:build-cache`：手动清理已识别的 Rust 中间缓存。直接运行 Cargo 后也可执行此命令。

清理限定在当前项目 `target` 的标准 debug / release、架构子目录和 `next-release-validation` 编译缓存内，拒绝符号链接或目录联接。保留安装包、顶层可执行文件、验证记录、账号数据与其他项目目录，不清理系统 Temp。清理后的缓存会在下次编译时重新生成，因此不会在每次成功构建后全部删除。
