//! MiniMax Code 进程探测 / 关闭 / 启动。
//!
//! 桌面端是 Electron（Windows 映像名 `MiniMax Code.exe`），可以直接按映像名找；
//! CLI `mcode` 跑在 Node 运行时里，只能按命令行识别，因此只做「检测到就拒绝切换」，
//! 不自动结束用户的终端进程。
use std::path::PathBuf;
use std::time::Duration;

use crate::modules::process;

#[cfg(target_os = "macos")]
use super::paths::macos_process_marker;
#[allow(unused_imports)]
use super::paths::{desktop_image, Paths, Region};

pub trait Processes: Send + Sync {
    fn running(&self, region: Region) -> Result<bool, String>;
    /// CLI 与桌面端共用同一份登录态；它在运行时切换会被退出时写回覆盖。
    fn cli_running(&self) -> Result<bool, String>;
    fn running_exe(&self) -> Option<PathBuf>;
    fn close(&self, region: Region, timeout: Duration) -> Result<(), String>;
    fn launch(&self, paths: &Paths, region: Region) -> Result<(), String>;
}

pub struct NativeProcesses;

#[cfg(windows)]
fn desktop_pids() -> Vec<u32> {
    process::windows_tasklist_image_rows(desktop_image())
        .iter()
        .filter(|row| row.pid != std::process::id())
        .map(|row| row.pid)
        .collect()
}

#[cfg(target_os = "macos")]
fn desktop_pids() -> Vec<u32> {
    process::macos_rows_by_patterns(&[macos_process_marker().to_string()])
        .into_iter()
        .map(|(pid, _)| pid)
        .filter(|pid| *pid != std::process::id())
        .collect()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn desktop_pids() -> Vec<u32> {
    Vec::new()
}

#[cfg(windows)]
fn cli_pids() -> Result<Vec<u32>, String> {
    let script = "Get-CimInstance Win32_Process -Filter \"Name='node.exe'\" | \
         Where-Object { $_.CommandLine -match '@minimax-ai[\\\\/]code' -or \
         $_.CommandLine -match '[\\\\/]minimax-code[\\\\/]' } | \
         ForEach-Object { $_.ProcessId }";
    let stdout = process::ps_output(script, 10).ok_or("无法检测 mcode CLI 进程")?;
    Ok(stdout
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .filter(|pid| *pid != std::process::id())
        .collect())
}

#[cfg(target_os = "macos")]
fn cli_pids() -> Result<Vec<u32>, String> {
    let output = process::run_cmd_timeout("/bin/ps", &["-axo", "pid=,command="], 10)
        .ok_or("无法检测 mcode CLI 进程")?;
    if !output.status.success() {
        return Err("无法检测 mcode CLI 进程".into());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains("@minimax-ai/code") || line.contains("/minimax-code/"))
        .filter_map(|line| line.split_whitespace().next()?.parse::<u32>().ok())
        .filter(|pid| *pid != std::process::id())
        .collect())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn cli_pids() -> Result<Vec<u32>, String> {
    Ok(Vec::new())
}

impl Processes for NativeProcesses {
    fn running(&self, region: Region) -> Result<bool, String> {
        let _ = region;
        #[cfg(windows)]
        {
            // 只问目标映像，不必把整张进程表取回来再过滤（列表越大越慢）。
            // 查询本身失败仍然报错：`status` 会把它显示成「状态未知」，
            // 而切换路径宁可报错也不要在客户端还开着的时候硬写凭据。
            let filter = format!("IMAGENAME eq {}", desktop_image());
            let output =
                process::run_cmd_timeout("tasklist", &["/FI", &filter, "/FO", "CSV", "/NH"], 10)
                    .ok_or("无法可靠查询 MiniMax Code 进程")?;
            if !output.status.success() {
                return Err("无法可靠查询 MiniMax Code 进程".into());
            }
            let rows = process::parse_tasklist_csv(&String::from_utf8_lossy(&output.stdout));
            Ok(!rows.is_empty())
        }
        #[cfg(not(windows))]
        {
            Ok(!desktop_pids().is_empty())
        }
    }

    fn cli_running(&self) -> Result<bool, String> {
        Ok(!cli_pids()?.is_empty())
    }

    fn running_exe(&self) -> Option<PathBuf> {
        #[cfg(windows)]
        {
            let script = "Get-Process -Name 'MiniMax Code' -ErrorAction SilentlyContinue | \
                 ForEach-Object { $p = ''; try { $p = $_.Path } catch {}; '{{0}}|{{1}}|{{2}}' -f $_.Id, $_.ProcessName, $p }";
            let stdout = process::ps_output(script, 5)?;
            process::parse_windows_process_rows(&stdout)
                .into_iter()
                .filter_map(|row| row.exe_path)
                .find(|path| path.is_file())
        }
        #[cfg(target_os = "macos")]
        {
            process::macos_rows_by_patterns(&[macos_process_marker().to_string()])
                .into_iter()
                .find_map(|(_, args)| process::extract_app_bundle_from_args(&args))
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            None
        }
    }

    fn close(&self, region: Region, timeout: Duration) -> Result<(), String> {
        let _ = region;
        let pids = desktop_pids();
        if pids.is_empty() {
            return Ok(());
        }
        #[cfg(windows)]
        {
            process::windows_send_close_to_pids(&pids);
            let remaining = process::wait_windows_pids_gone(&pids, Duration::from_secs(8));
            for pid in &remaining {
                let _ = process::run_cmd_timeout(
                    "taskkill",
                    &["/PID", &pid.to_string(), "/T", "/F"],
                    10,
                );
            }
            let leftover = process::wait_windows_pids_gone(&remaining, timeout);
            if !leftover.is_empty() {
                return Err(format!(
                    "MiniMax Code 无法完全关闭（残留进程: {leftover:?}），请手动退出后重试"
                ));
            }
        }
        #[cfg(not(windows))]
        {
            let deadline = std::time::Instant::now() + timeout;
            while Instant::now() < deadline {
                if desktop_pids().is_empty() {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            if !desktop_pids().is_empty() {
                return Err("MiniMax Code 仍在运行，请手动退出后重试".into());
            }
        }
        Ok(())
    }

    fn launch(&self, paths: &Paths, region: Region) -> Result<(), String> {
        let exe = self
            .running_exe()
            .or_else(|| paths.executable(region))
            .ok_or_else(|| "未找到 MiniMax Code 客户端，请先安装或手动打开".to_string())?;
        #[cfg(windows)]
        {
            process::cmd_builder(&exe)
                .spawn()
                .map(|_| ())
                .map_err(|_| "无法启动 MiniMax Code，请手动打开".to_string())
        }
        #[cfg(target_os = "macos")]
        {
            process::cmd_builder("/usr/bin/open")
                .arg(&exe)
                .spawn()
                .map(|_| ())
                .map_err(|_| "无法启动 MiniMax Code，请手动打开".to_string())
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = exe;
            Err("不支持此系统".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::modules::process::parse_tasklist_csv;

    /// 带 `/FI` 过滤时，没有匹配进程会输出一行提示（中文系统上是 GBK，
    /// 到 Rust 这边已被回转成替换字符），它不能被当成「有一个进程」。
    #[test]
    fn filtered_tasklist_hint_line_is_not_a_process() {
        assert!(parse_tasklist_csv("信息: 没有运行的任务匹配指定标准。").is_empty());
        assert!(parse_tasklist_csv(
            "INFO: No tasks are running which match the specified criteria."
        )
        .is_empty());
    }

    /// 真正命中时要能解析出 PID：`running()` 现在完全依赖「有没有行」。
    #[test]
    fn filtered_tasklist_row_yields_the_pid() {
        let rows =
            parse_tasklist_csv("\"MiniMax Code.exe\",\"1234\",\"Console\",\"2\",\"120,000 K\"");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].pid, 1234);
        assert!(rows[0].name.eq_ignore_ascii_case("MiniMax Code.exe"));
    }
}
