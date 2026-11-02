//! Install the bundled CLI for this user on macOS and AppImage. Debian already
//! installs the sidecar in /usr/bin. Never overwrite an unrelated command.
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

const NAME: &str = "workbuddy-switch-next";
const MARKER: &str = "workbuddy-switch-next managed cli v1\n";
const LAUNCHER: &str = "#!/bin/sh\n# WorkBuddy Switch Next managed CLI\nexec \"$HOME/.local/share/workbuddy-switch-next/cli/workbuddy-switch-next\" \"$@\"\n";
const START: &str = "# >>> WorkBuddy Switch Next CLI >>>";
const SHELL_BLOCK: &str = "\n# >>> WorkBuddy Switch Next CLI >>>\ncase \":${PATH-}:\" in *\":$HOME/.local/bin:\"*) ;; *) export PATH=\"${PATH:+$PATH:}$HOME/.local/bin\" ;; esac\n# <<< WorkBuddy Switch Next CLI <<<\n";
const FISH_BLOCK: &str = "# WorkBuddy Switch Next managed CLI\nif not contains -- $HOME/.local/bin $PATH\n    set -gx PATH $PATH $HOME/.local/bin\nend\n";

#[cfg(not(debug_assertions))]
pub fn install_for_current_user() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let source = exe
        .parent()
        .ok_or_else(|| io::Error::other("Executable directory missing"))?
        .join(NAME);
    // The Debian package installs both binaries in /usr/bin, already on PATH.
    if cfg!(target_os = "linux") && source == Path::new("/usr/bin").join(NAME) {
        return Ok(());
    }
    let home = std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME missing"))?;
    let home = Path::new(&home);
    if !home.is_absolute() {
        return Err(io::Error::other("HOME is not absolute"));
    }
    install(home, &source)
}

fn install(home: &Path, source: &Path) -> io::Result<()> {
    if !source.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Bundled CLI missing",
        ));
    }
    let bin = home.join(".local/bin");
    let launcher = bin.join(NAME);
    match fs::symlink_metadata(&launcher) {
        Ok(meta) if !meta.is_file() || fs::read(&launcher)? != LAUNCHER.as_bytes() => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "CLI command already exists",
            ));
        }
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let data = home.join(".local/share/workbuddy-switch-next/cli");
    let binary = data.join(NAME);
    let marker = data.join(".managed");
    fs::create_dir_all(&data)?;
    if let Ok(meta) = fs::symlink_metadata(&marker) {
        if !meta.is_file() || fs::read_to_string(&marker)? != MARKER {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "CLI ownership marker is not managed",
            ));
        }
    }
    if fs::symlink_metadata(&binary).is_ok()
        && fs::read_to_string(&marker).ok().as_deref() != Some(MARKER)
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Unmanaged CLI file already exists",
        ));
    }
    if let Ok(meta) = fs::symlink_metadata(&binary) {
        if !meta.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "CLI file is not a regular file",
            ));
        }
    }
    fs::write(&marker, MARKER)?;
    let temp = data.join(format!(
        ".cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let result = (|| {
        fs::copy(source, &temp)?;
        fs::rename(&temp, &binary)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result?;
    fs::create_dir_all(&bin)?;
    if !launcher.exists() {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&launcher)?;
        file.write_all(LAUNCHER.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755))?;
        }
    }
    let bash_login = if home.join(".bash_profile").exists() {
        ".bash_profile"
    } else {
        ".profile"
    };
    for profile in [bash_login, ".bashrc", ".zprofile", ".zshrc"] {
        append_shell_block(&home.join(profile))?;
    }
    let fish = home.join(".config/fish");
    if fish.is_dir()
        || std::env::var_os("SHELL").is_some_and(|shell| Path::new(&shell).ends_with("fish"))
    {
        let conf = fish.join("conf.d");
        fs::create_dir_all(&conf)?;
        let file = conf.join("workbuddy-switch-next.fish");
        match OpenOptions::new().write(true).create_new(true).open(file) {
            Ok(mut out) => out.write_all(FISH_BLOCK.as_bytes())?,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn append_shell_block(path: &Path) -> io::Result<()> {
    let current = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    if current
        .windows(START.len())
        .any(|part| part == START.as_bytes())
    {
        return Ok(());
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(SHELL_BLOCK.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let valid = fs::canonicalize(&self.0)
                .ok()
                .zip(fs::canonicalize(std::env::temp_dir()).ok())
                .is_some_and(|(dir, root)| {
                    dir.parent() == Some(root.as_path())
                        && dir.file_name().is_some_and(|name| {
                            name.to_string_lossy().starts_with("wb-cli-install-")
                        })
                });
            if valid {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }
    fn fixture() -> (Temp, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wb-cli-install-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source");
        fs::write(&source, b"cli-v1").unwrap();
        (Temp(dir), source)
    }
    #[test]
    fn install_is_idempotent_and_updates_its_own_binary() {
        let (tmp, source) = fixture();
        fs::write(tmp.0.join(".profile"), "existing\n").unwrap();
        install(&tmp.0, &source).unwrap();
        fs::write(&source, b"cli-v2").unwrap();
        install(&tmp.0, &source).unwrap();
        let profile = fs::read_to_string(tmp.0.join(".profile")).unwrap();
        assert_eq!(profile.matches(START).count(), 1);
        assert!(profile.starts_with("existing\n"));
        assert_eq!(
            fs::read(
                tmp.0
                    .join(".local/share/workbuddy-switch-next/cli/workbuddy-switch-next")
            )
            .unwrap(),
            b"cli-v2"
        );
    }
    #[test]
    fn does_not_replace_existing_user_command() {
        let (tmp, source) = fixture();
        let bin = tmp.0.join(".local/bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join(NAME), b"user command").unwrap();
        assert_eq!(
            install(&tmp.0, &source).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(bin.join(NAME)).unwrap(), b"user command");
        assert!(!tmp.0.join(".profile").exists());
    }

    #[cfg(unix)]
    #[test]
    fn new_shell_can_run_the_installed_command() {
        use std::os::unix::fs::PermissionsExt;
        let (tmp, source) = fixture();
        fs::write(&source, "#!/bin/sh\nprintf '%s' \"$1\"\n").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();
        install(&tmp.0, &source).unwrap();
        let output = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(". \"$HOME/.profile\"; workbuddy-switch-next hello")
            .env("HOME", &tmp.0)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"hello");
    }
}
