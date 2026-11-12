//! Replacing the running binary and restarting it (research R7).
//!
//! The old binary stays next to the new one as `putio-sync.old` (`putio-sync.old.exe` on
//! Windows) until the new version has started cleanly, so a failed start can put it back
//! (see [`crate::trial`]).

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Why the swap didn't happen. The running binary is unchanged after any of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwapError {
    /// The binary's folder can't be written (US3 scenario 6)
    NotWritable(PathBuf),
    Io(String),
}

impl std::fmt::Display for SwapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotWritable(path) => write!(f, "{} can't be written", path.display()),
            Self::Io(e) => write!(f, "could not replace the binary: {e}"),
        }
    }
}

impl std::error::Error for SwapError {}

fn io(e: std::io::Error) -> SwapError {
    SwapError::Io(e.to_string())
}

/// Where the old binary waits during the trial: `putio-sync.old`, or `putio-sync.old.exe`
pub fn old_path(exe: &Path) -> PathBuf {
    let stem = exe
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "putio-sync".into());
    let name = match exe.extension() {
        Some(ext) => format!("{stem}.old.{}", ext.to_string_lossy()),
        None => format!("{stem}.old"),
    };
    exe.with_file_name(name)
}

fn new_path(exe: &Path) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".new");
    exe.with_file_name(name)
}

/// The binary's folder can be written, or [`SwapError::NotWritable`]
pub fn check_writable(exe: &Path) -> Result<(), SwapError> {
    let dir = exe.parent().unwrap_or(Path::new("."));
    match tempfile::Builder::new()
        .prefix(".putio-sync-write-check-")
        .tempfile_in(dir)
    {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(SwapError::NotWritable(dir.to_path_buf()))
        }
        Err(e) => Err(io(e)),
    }
}

/// Put `bytes` in place of `exe`, keeping the old binary at [`old_path`]
pub fn install(bytes: &[u8], exe: &Path) -> Result<(), SwapError> {
    install_with(bytes, exe, &mut |_| {})
}

/// [`install`], calling `step` after each filesystem change (tests list the folder there)
pub(crate) fn install_with(
    bytes: &[u8],
    exe: &Path,
    step: &mut dyn FnMut(&str),
) -> Result<(), SwapError> {
    check_writable(exe)?;
    let old = old_path(exe);
    let new = new_path(exe);
    let _ = std::fs::remove_file(&new);
    let _ = std::fs::remove_file(&old);
    let result = imp::install(bytes, exe, &new, &old, step);
    if result.is_err() {
        let _ = std::fs::remove_file(&new);
    }
    result
}

fn write_new(bytes: &[u8], exe: &Path, new: &Path) -> Result<(), SwapError> {
    let mut file = std::fs::File::create(new).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => {
            SwapError::NotWritable(exe.parent().unwrap_or(Path::new(".")).to_path_buf())
        }
        _ => io(e),
    })?;
    file.write_all(bytes).map_err(io)?;
    if let Ok(meta) = std::fs::metadata(exe) {
        file.set_permissions(meta.permissions()).map_err(io)?;
    }
    file.sync_all().map_err(io)
}

#[cfg(unix)]
mod imp {
    use super::*;

    /// Write `.new`, keep a hard link (or a copy) of the old binary as `.old`, then rename
    /// `.new` over the binary: the name always points at a whole binary
    pub(super) fn install(
        bytes: &[u8],
        exe: &Path,
        new: &Path,
        old: &Path,
        step: &mut dyn FnMut(&str),
    ) -> Result<(), SwapError> {
        write_new(bytes, exe, new)?;
        step("new written");
        if std::fs::hard_link(exe, old).is_err() {
            std::fs::copy(exe, old).map_err(io)?;
        }
        step("old kept");
        std::fs::rename(new, exe).map_err(io)?;
        step("new in place");
        if let Some(dir) = exe.parent() {
            if let Ok(dir) = std::fs::File::open(dir) {
                let _ = dir.sync_all();
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
mod imp {
    use super::*;

    /// Windows allows renaming a running exe but not replacing it: move it aside, then put
    /// the new file under its name
    pub(super) fn install(
        bytes: &[u8],
        exe: &Path,
        new: &Path,
        old: &Path,
        step: &mut dyn FnMut(&str),
    ) -> Result<(), SwapError> {
        write_new(bytes, exe, new)?;
        step("new written");
        std::fs::rename(exe, old).map_err(io)?;
        step("old moved");
        if let Err(e) = std::fs::rename(new, exe) {
            let _ = std::fs::rename(old, exe);
            return Err(io(e));
        }
        step("new in place");
        Ok(())
    }
}

/// Put the old binary back in place of `exe` (a rollback)
pub fn restore(old: &Path, exe: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        // The failed new exe may be the running one: move it aside first
        let failed = exe.with_extension("failed.exe");
        let _ = std::fs::remove_file(&failed);
        if exe.exists() {
            std::fs::rename(exe, &failed)?;
        }
    }
    std::fs::rename(old, exe)
}

/// Run `exe` in place of this process with the same arguments and environment. On Unix the
/// process is replaced (`exec`), so the PID and the supervisor stay; on Windows the new
/// process is started detached and this one exits. Returns only on failure.
pub fn restart(exe: &Path, args: Vec<OsString>) -> std::io::Error {
    tracing::info!(exe = %exe.display(), "Restarting");
    imp_restart(exe, args)
}

/// [`restart`] the binary at the path this process was started from
pub fn restart_self() -> std::io::Error {
    match std::env::current_exe() {
        Ok(exe) => restart(&exe, std::env::args_os().skip(1).collect()),
        Err(e) => e,
    }
}

#[cfg(unix)]
fn imp_restart(exe: &Path, args: Vec<OsString>) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    std::process::Command::new(exe).args(args).exec()
}

#[cfg(windows)]
fn imp_restart(exe: &Path, args: Vec<OsString>) -> std::io::Error {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    match std::process::Command::new(exe)
        .args(args)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
    {
        Ok(_) => std::process::exit(0),
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn old_path_keeps_the_extension_last() {
        assert_eq!(
            old_path(Path::new("/opt/putio-sync")),
            Path::new("/opt/putio-sync.old")
        );
        assert_eq!(
            old_path(Path::new("C:/p/putio-sync.exe")),
            Path::new("C:/p/putio-sync.old.exe")
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_keeps_the_old_binary_and_the_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("putio-sync");
        std::fs::write(&exe, "#!/bin/sh\necho old\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o750)).unwrap();

        let mut seen = Vec::new();
        install_with(b"#!/bin/sh\necho new\n", &exe, &mut |step| {
            let names = names(dir.path());
            assert!(
                names.contains(&"putio-sync".to_string()),
                "{step}: {names:?}"
            );
            seen.push(step.to_string());
        })
        .unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(names(dir.path()), ["putio-sync", "putio-sync.old"]);
        assert_eq!(std::fs::read(&exe).unwrap(), b"#!/bin/sh\necho new\n");
        assert_eq!(
            std::fs::read(dir.path().join("putio-sync.old")).unwrap(),
            b"#!/bin/sh\necho old\n"
        );
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&exe), 0o750);
        assert_eq!(mode(&dir.path().join("putio-sync.old")), 0o750);

        // a second update replaces the leftover .old
        install(b"newer", &exe).unwrap();
        assert_eq!(names(dir.path()), ["putio-sync", "putio-sync.old"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_folder_is_not_writable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("putio-sync");
        std::fs::write(&exe, "old").unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let result = install(b"new", &exe);
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            result,
            Err(SwapError::NotWritable(dir.path().to_path_buf()))
        );
        assert_eq!(names(dir.path()), ["putio-sync"]);
        assert_eq!(std::fs::read(&exe).unwrap(), b"old");
    }

    #[test]
    fn restore_puts_the_old_binary_back() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("putio-sync");
        std::fs::write(&exe, "old").unwrap();
        install(b"new", &exe).unwrap();
        restore(&old_path(&exe), &exe).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"old");
        assert!(!old_path(&exe).exists());
    }

    /// The running exe can be renamed aside while a process holds it open
    #[cfg(windows)]
    #[test]
    fn install_moves_a_running_exe_aside() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("putio-sync.exe");
        std::fs::copy(std::env::var_os("ComSpec").unwrap(), &exe).unwrap();
        let mut child = std::process::Command::new(&exe)
            .args(["/c", "ping", "-n", "5", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        install(b"new", &exe).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert!(dir.path().join("putio-sync.old.exe").exists());
        child.kill().unwrap();
        let _ = child.wait();
    }
}
