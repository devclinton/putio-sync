#![allow(dead_code)]

use std::path::Path;

use putio_sync_core::events::{Event, HookTest, Payload};
use putio_sync_core::hooks::HookConfig;

/// Write an executable `#!/bin/sh` script.
#[cfg(unix)]
pub fn script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{}\n", body)).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

pub fn command_hook(name: &str, program: &Path) -> HookConfig {
    HookConfig {
        name: name.to_string(),
        events: vec!["*".to_string()],
        command: Some(vec![program.display().to_string()]),
        ..Default::default()
    }
}

pub fn url_hook(name: &str, url: &str) -> HookConfig {
    HookConfig {
        name: name.to_string(),
        events: vec!["*".to_string()],
        url: Some(url.to_string()),
        timeout_secs: 2,
        ..Default::default()
    }
}

pub fn test_event() -> Event {
    Event::new(Payload::HookTest(HookTest {
        hook: "t".to_string(),
        message: "hi".to_string(),
    }))
}
