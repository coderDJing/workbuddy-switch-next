use super::paths::Paths;
#[cfg(any(windows, target_os = "macos"))]
use crate::modules::process;
use std::time::{Duration, Instant};

pub(super) trait Processes: Send + Sync {
    fn running(&self) -> Result<bool, String>;
    fn close(&self) -> Result<(), String>;
    fn launch(&self, paths: &Paths) -> Result<(), String>;
}
pub(super) struct NativeProcesses;
fn pids() -> Result<Vec<u32>, String> {
    #[cfg(windows)]
    {
        let out = process::run_cmd_timeout(
            "tasklist",
            &["/FI", "IMAGENAME eq Doubao.exe", "/FO", "CSV", "/NH"],
            10,
        )
        .ok_or("无法检测豆包进程")?;
        if !out.status.success() {
            return Err("无法检测豆包进程".into());
        }
        Ok(
            process::parse_tasklist_csv(&String::from_utf8_lossy(&out.stdout))
                .into_iter()
                .map(|r| r.pid)
                .collect(),
        )
    }
    #[cfg(target_os = "macos")]
    {
        let out = process::run_cmd_timeout("/bin/ps", &["-axo", "pid=,command="], 10)
            .ok_or("无法检测豆包进程")?;
        if !out.status.success() {
            return Err("无法检测豆包进程".into());
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.contains("Doubao.app/Contents/") || l.contains("豆包.app/Contents/"))
            .filter_map(|l| l.split_whitespace().next()?.parse().ok())
            .collect())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Err("豆包仅支持 Windows x64 与 macOS Apple Silicon".into())
    }
}
impl Processes for NativeProcesses {
    fn running(&self) -> Result<bool, String> {
        Ok(!pids()?.is_empty())
    }
    fn close(&self) -> Result<(), String> {
        let running = pids()?;
        if running.is_empty() {
            return Ok(());
        }
        #[cfg(windows)]
        process::windows_send_close_to_pids(&running);
        #[cfg(target_os = "macos")]
        {
            let _ = process::run_cmd_timeout(
                "/usr/bin/osascript",
                &["-e", "tell application \"Doubao\" to quit"],
                5,
            );
        }
        let until = Instant::now() + Duration::from_secs(20);
        while Instant::now() < until {
            if pids()?.is_empty() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Err("豆包尚未完全退出，请保存工作并从托盘退出后重试".into())
    }
    fn launch(&self, paths: &Paths) -> Result<(), String> {
        let exe = paths
            .executable()
            .ok_or("未找到豆包客户端，请先安装或手动打开豆包")?;
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            std::process::Command::new(exe)
                .creation_flags(0x08000000)
                .spawn()
                .map_err(|_| "无法启动豆包")?;
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("/usr/bin/open")
                .arg(exe)
                .spawn()
                .map_err(|_| "无法启动豆包")?;
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        let _ = exe;
        Ok(())
    }
}
