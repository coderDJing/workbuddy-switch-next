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
/// 只发送礼貌关闭请求，不强杀：未保存的窗口留给用户处理。
#[cfg(windows)]
fn request_close(ids: &[u32]) {
    // 豆包是多进程：只要主窗口还有可见窗口，SendClose 就会返回大于 0，
    // 但托盘/后台进程没有顶层窗口，WM_CLOSE 永远到不了它们。
    // 因此按 PID 分类，只对「无窗口」的那部分补发一次不带 /F 的 taskkill。
    let with_window = process::windows_visible_window_pids(ids);
    process::windows_send_close_to_pids(ids);
    let headless: Vec<u32> = ids
        .iter()
        .copied()
        .filter(|p| !with_window.contains(p))
        .collect();
    if headless.is_empty() {
        return;
    }
    let mut args = Vec::with_capacity(headless.len() * 2 + 1);
    for pid in &headless {
        args.push("/PID".to_string());
        args.push(pid.to_string());
    }
    args.push("/T".to_string());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    // 一次调用处理全部无窗口进程，避免逐个 10 秒超时累加。
    let _ = process::run_cmd_timeout("taskkill", &refs, 10);
}
#[cfg(target_os = "macos")]
fn request_close(_ids: &[u32]) {
    let _ = process::run_cmd_timeout(
        "/usr/bin/osascript",
        &["-e", "tell application \"Doubao\" to quit"],
        5,
    );
}
#[cfg(not(any(windows, target_os = "macos")))]
fn request_close(_ids: &[u32]) {}
impl Processes for NativeProcesses {
    fn running(&self) -> Result<bool, String> {
        Ok(!pids()?.is_empty())
    }
    fn close(&self) -> Result<(), String> {
        let running = pids()?;
        if running.is_empty() {
            return Ok(());
        }
        let until = Instant::now() + Duration::from_secs(30);
        request_close(&running);
        // 之后只观察进程是否退出：重复发 WM_CLOSE 会反复启动 PowerShell，
        // 也可能干扰用户处理「未保存内容」确认框。
        while Instant::now() < until {
            if pids()?.is_empty() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        // 关闭请求本身可能耗时较久，超时前再确认一次，避免误报。
        if pids()?.is_empty() {
            return Ok(());
        }
        Err("豆包尚未完全退出，请保存工作，从托盘图标右键彻底退出豆包后重试".into())
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
