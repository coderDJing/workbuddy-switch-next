use super::paths::{Client, Paths, Region};
use crate::modules::process;

pub trait Processes: Send + Sync {
    fn running(&self, region: Region, client: Client) -> Result<bool, String>;
    fn close(&self, region: Region, client: Client) -> Result<(), String>;
    fn launch(&self, paths: &Paths, region: Region, client: Client) -> Result<(), String>;
}
pub struct NativeProcesses;
impl Processes for NativeProcesses {
    fn running(&self, region: Region, client: Client) -> Result<bool, String> {
        let name = client.app_name(region);
        #[cfg(windows)]
        {
            let image = format!("{name}.exe");
            let output = process::run_cmd_timeout("tasklist", &["/FO", "CSV", "/NH"], 10)
                .ok_or("无法可靠查询 TRAE 进程")?;
            if !output.status.success() {
                return Err("无法可靠查询 TRAE 进程".into());
            }
            let rows = process::parse_tasklist_csv(&String::from_utf8_lossy(&output.stdout));
            if rows.is_empty() {
                return Err("TRAE 进程列表无法解析，未修改登录态".into());
            }
            Ok(rows.iter().any(|p| p.name.eq_ignore_ascii_case(&image)))
        }
        #[cfg(target_os = "macos")]
        {
            let output = process::run_cmd_timeout("/bin/ps", &["-axo", "comm="], 10)
                .ok_or("无法查询 TRAE 进程")?;
            if !output.status.success() {
                return Err("无法查询 TRAE 进程".into());
            }
            let marker = format!("/{name}.app/");
            Ok(String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|p| p.contains(&marker)))
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = name;
            Err("TRAE 接入不支持此系统".into())
        }
    }
    fn close(&self, region: Region, client: Client) -> Result<(), String> {
        let name = client.app_name(region);
        #[cfg(windows)]
        {
            let pids: Vec<_> = process::windows_tasklist_image_rows(&format!("{name}.exe"))
                .iter()
                .map(|p| p.pid)
                .collect();
            process::windows_send_close_to_pids(&pids);
        }
        #[cfg(target_os = "macos")]
        {
            let script = format!("tell application \"{name}\" to quit");
            let result = process::run_cmd_timeout("/usr/bin/osascript", &["-e", &script], 15)
                .ok_or("TRAE 关闭请求超时")?;
            if !result.status.success() {
                return Err("TRAE 未同意退出，未修改登录态".into());
            }
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while self.running(region, client)? {
            if std::time::Instant::now() >= deadline {
                return Err("TRAE 仍在运行，请保存任务并手动退出后重试".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        Ok(())
    }
    fn launch(&self, paths: &Paths, region: Region, client: Client) -> Result<(), String> {
        let exe = paths
            .executable(region, client)
            .ok_or("未找到 TRAE 客户端，请先安装并启动一次")?;
        #[cfg(windows)]
        {
            process::cmd_builder(exe)
                .spawn()
                .map_err(|_| "无法启动 TRAE")?;
        }
        #[cfg(target_os = "macos")]
        {
            process::cmd_builder("/usr/bin/open")
                .arg(exe)
                .spawn()
                .map_err(|_| "无法启动 TRAE")?;
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = exe;
            return Err("不支持此系统".into());
        }
        #[cfg(any(windows, target_os = "macos"))]
        Ok(())
    }
}
