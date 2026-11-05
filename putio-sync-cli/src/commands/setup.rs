//! `putio-sync setup` (feature 013): the setup wizard in a terminal, for headless
//! installs. Asks for what the web wizard asks for (Radarr/Sonarr aside), runs the dry
//! run, and writes the config once.

use anyhow::{anyhow, bail, Context, Result};
use putio_sync_api::extract_download_config;
use putio_sync_api::setup::new_api_token;
use putio_sync_client::oob::{DeviceLink, OobCheck, LINK_URL};
use putio_sync_client::{PutioApiClient, DEFAULT_BASE_URL};
use putio_sync_core::config::Config;
use putio_sync_core::setup::{
    check_folder, check_local, check_preset, preset_choices, putio_token, resolve_app_id,
    suggest_download_dir, FolderWarning, SetupChoices,
};
use putio_sync_download::{dry_run, DRY_RUN_LIMIT};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long to wait for the device link before giving up
const LINK_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const LINK_POLL: Duration = Duration::from_secs(3);
/// Samples printed from the dry run
const SAMPLES: usize = 20;

/// Values given as flags
#[derive(Debug, Default, Clone)]
pub struct SetupArgs {
    pub download_dir: Option<PathBuf>,
    pub preset: Option<String>,
    pub watch_dir: Option<PathBuf>,
    pub metrics: Option<bool>,
    pub no_dry_run: bool,
    pub yes: bool,
}

/// Where the answers come from: the terminal, or a script in tests. `None` means the
/// input ended (Ctrl-D), which cancels.
pub trait Prompter {
    /// A person is answering; otherwise every suggestion is taken
    fn interactive(&self) -> bool;
    /// Ask with `default` shown; Enter takes it
    fn ask(&mut self, question: &str, default: &str) -> Result<Option<String>>;
    /// One line holding the Put.io token
    fn token_line(&mut self, prompt: &str) -> Result<Option<String>>;
}

/// Prompts on stderr, answers from stdin
pub struct TerminalPrompter {
    tty: bool,
}

impl TerminalPrompter {
    pub fn new() -> Self {
        Self {
            tty: io::stdin().is_terminal(),
        }
    }

    fn line(&self) -> Result<Option<String>> {
        let mut line = String::new();
        if io::stdin().lock().read_line(&mut line)? == 0 {
            return Ok(None);
        }
        Ok(Some(line.trim().to_string()))
    }
}

impl Default for TerminalPrompter {
    fn default() -> Self {
        Self::new()
    }
}

impl Prompter for TerminalPrompter {
    fn interactive(&self) -> bool {
        self.tty
    }

    fn ask(&mut self, question: &str, default: &str) -> Result<Option<String>> {
        if default.is_empty() {
            eprint!("{question}: ");
        } else {
            eprint!("{question} [{default}]: ");
        }
        io::stderr().flush()?;
        Ok(self
            .line()?
            .map(|a| if a.is_empty() { default.to_string() } else { a }))
    }

    /// On a terminal the token is read with echo off, so it never shows on screen
    fn token_line(&mut self, prompt: &str) -> Result<Option<String>> {
        if !self.tty {
            return self.line();
        }
        match rpassword::prompt_password(format!("{prompt} (hidden): ")) {
            Ok(token) => Ok(Some(token.trim().to_string())),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::UnexpectedEof | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(e).context("could not read the token from the terminal"),
        }
    }
}

/// What the run ended with
#[derive(Debug, PartialEq, Eq)]
pub enum Finished {
    /// The config was written
    Written,
    /// The user said no, or the input ended
    Cancelled,
}

/// What the environment supplies, so tests need not set variables
pub struct SetupEnv {
    /// `PUTIO_SYNC_PUTIO_TOKEN`
    pub putio_token: Option<String>,
    /// `PUTIO_SYNC_API_TOKEN` is set
    pub api_token_in_env: bool,
    /// Put.io API base URL
    pub putio_base_url: String,
}

impl SetupEnv {
    fn from_process() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        Self {
            putio_token: var("PUTIO_SYNC_PUTIO_TOKEN"),
            api_token_in_env: var("PUTIO_SYNC_API_TOKEN").is_some(),
            putio_base_url: DEFAULT_BASE_URL.to_string(),
        }
    }
}

/// Run `putio-sync setup`; returns the exit code (0 written, 1 error, 2 cancelled)
pub async fn execute_setup(args: SetupArgs, config_path: Option<PathBuf>) -> i32 {
    let path = match config_path.or_else(|| Config::default_path().ok()) {
        Some(path) => path,
        None => {
            eprintln!("Error: could not determine the config file path; pass --config");
            return 1;
        }
    };
    let mut prompter = TerminalPrompter::new();
    let mut out = io::stdout();
    match run_setup(
        &path,
        &args,
        &SetupEnv::from_process(),
        &mut prompter,
        &mut out,
    )
    .await
    {
        Ok(Finished::Written) => 0,
        Ok(Finished::Cancelled) => {
            eprintln!("Cancelled; nothing was written.");
            2
        }
        Err(e) => {
            eprintln!("Error: {e:#}");
            1
        }
    }
}

macro_rules! say {
    ($out:expr, $($arg:tt)*) => { writeln!($out, $($arg)*)? };
}

/// The setup flow, with its input and output passed in
pub async fn run_setup(
    path: &Path,
    args: &SetupArgs,
    env: &SetupEnv,
    prompter: &mut dyn Prompter,
    out: &mut dyn Write,
) -> Result<Finished> {
    // Without a person to ask, every suggestion is taken
    let ask = prompter.interactive() && !args.yes;
    let local = check_local(path);
    let base = local.config.clone().unwrap_or_default();
    say!(out, "Setting up putio-sync in {}", path.display());
    for reason in &local.reasons {
        say!(out, "  - {}", reason.describe());
    }
    let mut choices = SetupChoices::default();

    // Put.io
    let (token, username, used) = match connect_putio(&base, env, prompter, ask, out).await? {
        Some(found) => found,
        None => return Ok(Finished::Cancelled),
    };
    say!(out, "Connected to Put.io as {username}.");
    choices.putio_token = Some(token.clone());

    // Download folder
    let suggested = suggest_download_dir(local.config.as_ref());
    let mut answer = args.download_dir.clone();
    let dir = loop {
        let dir = match answer.take() {
            Some(dir) => dir,
            None if ask => {
                match prompter.ask("Download folder", &suggested.display().to_string())? {
                    Some(a) => PathBuf::from(a),
                    None => return Ok(Finished::Cancelled),
                }
            }
            None => suggested.clone(),
        };
        let check = check_folder(&dir, Some(used), false);
        for warning in &check.warnings {
            say!(out, "  Note: {}", describe_warning(warning));
        }
        if check.usable() {
            if let Some(free) = check.free_bytes {
                say!(out, "  {} free.", human_bytes(free));
            }
            break dir;
        }
        let why = check
            .error
            .unwrap_or_else(|| "it is not writable".to_string());
        if !ask {
            bail!(
                "the download folder {} can't be used: {why} (set --download-dir)",
                dir.display()
            );
        }
        say!(out, "  {} can't be used: {why}", dir.display());
    };
    choices.download_dir = Some(dir);

    // What to download
    let presets = preset_choices(&base.download);
    let preset = match &args.preset {
        Some(name) => Some(name.clone()),
        None if ask => {
            say!(out, "What to download, for the whole account:");
            for (name, label) in &presets {
                say!(out, "  {name:<12} {label}");
            }
            let current = base.download.preset.clone().unwrap_or_default();
            match prompter.ask(
                "Preset (blank for everything except blocked file types)",
                &current,
            )? {
                Some(a) if a.is_empty() => None,
                Some(a) => Some(a),
                None => return Ok(Finished::Cancelled),
            }
        }
        None => None,
    };
    if let Some(name) = &preset {
        check_preset(&base.download, name).map_err(|e| anyhow!("{e} (set --preset)"))?;
    }
    choices.preset = preset;

    // Extras
    choices.watch_dir = match &args.watch_dir {
        Some(dir) => Some(Some(dir.clone())),
        None if ask => {
            let current = base
                .watch
                .enabled
                .then(|| base.watch.paths.first().map(|p| p.display().to_string()))
                .flatten()
                .unwrap_or_default();
            match prompter.ask(
                "Watch folder for .torrent and .magnet files (blank for none)",
                &current,
            )? {
                Some(a) if a.is_empty() => Some(None),
                Some(a) => Some(Some(PathBuf::from(a))),
                None => return Ok(Finished::Cancelled),
            }
        }
        None => None,
    };
    if let Some(Some(dir)) = &choices.watch_dir {
        let check = check_folder(dir, None, false);
        if !check.usable() {
            bail!(
                "the watch folder {} can't be used: {}",
                dir.display(),
                check
                    .error
                    .unwrap_or_else(|| "it is not writable".to_string())
            );
        }
    }
    choices.metrics = match args.metrics {
        Some(on) => Some(on),
        None if ask => {
            let default = if base.metrics.enabled { "y" } else { "n" };
            match prompter.ask(
                &format!("Prometheus metrics on {}? (y/n)", base.metrics.bind),
                default,
            )? {
                Some(a) => Some(yes(&a)),
                None => return Ok(Finished::Cancelled),
            }
        }
        None => None,
    };

    // An API token, unless one is configured
    let configured = base
        .resolve_api()
        .ok()
        .and_then(|r| r.value.token)
        .is_some();
    let created = (!configured && !env.api_token_in_env).then(new_api_token);
    choices.api_token = created.clone();

    let mut config = base.clone();
    choices.apply(&mut config).map_err(|e| anyhow!("{e}"))?;

    // Dry run
    if !args.no_dry_run {
        say!(
            out,
            "Dry run: listing your Put.io files (up to a minute)..."
        );
        let client = PutioApiClient::with_base_url(token, env.putio_base_url.clone());
        match dry_run(&client, &extract_download_config(&config), DRY_RUN_LIMIT).await {
            Ok(r) => {
                say!(
                    out,
                    "  {} files, {}, would be downloaded; {} already here; {} skipped by the rules.",
                    r.to_download.count,
                    human_bytes(r.to_download.bytes),
                    r.already_present.count,
                    r.skipped.count
                );
                if r.partial {
                    say!(
                        out,
                        "  Stopped after a minute; the counts cover what was listed."
                    );
                }
                for item in r.samples.iter().take(SAMPLES) {
                    say!(out, "    {}  {}", item.path, human_bytes(item.size));
                }
            }
            Err(e) => say!(out, "  The dry run failed: {e}"),
        }
    }

    if ask {
        match prompter.ask("Write config? (Y/n)", "y")? {
            Some(a) if yes(&a) => {}
            _ => return Ok(Finished::Cancelled),
        }
    }

    let dir = config.download.target_directory.clone();
    let check = check_folder(&dir, None, true);
    if !check.usable() || !dir.is_dir() {
        bail!(
            "could not create the download folder {}: {}",
            dir.display(),
            check.error.unwrap_or_else(|| "not writable".to_string())
        );
    }
    config
        .save_setup(path, created.is_some())
        .with_context(|| format!("could not write {}", path.display()))?;
    say!(out, "Saved {}.", path.display());
    if let Some(token) = created {
        say!(out, "Your API token, for the web UI and the API (shown once; it is saved as [api] auth_token):");
        say!(out, "  {token}");
    }
    say!(
        out,
        "Radarr and Sonarr can be connected later in the web UI: Settings → Radarr/Sonarr."
    );
    say!(
        out,
        "A daemon waiting in setup mode starts syncing within a few seconds."
    );
    Ok(Finished::Written)
}

/// The token, username and bytes used, or `None` when cancelled
async fn connect_putio(
    base: &Config,
    env: &SetupEnv,
    prompter: &mut dyn Prompter,
    ask: bool,
    out: &mut dyn Write,
) -> Result<Option<(String, String, u64)>> {
    let probe = |token: String| async move {
        let client = PutioApiClient::with_base_url(token.clone(), env.putio_base_url.clone());
        let probe = client.probe_account().await;
        if probe.unauthorized() {
            return Err(anyhow!("Put.io refused the token"));
        }
        let info = probe
            .result
            .map_err(|e| anyhow!("Put.io could not be reached: {e}"))?;
        Ok((
            token,
            info.username,
            u64::try_from(info.disk.used).unwrap_or(0),
        ))
    };

    if let Some(token) = &env.putio_token {
        return probe(token.trim().to_string()).await.map(Some);
    }
    // A working token already in the file is kept, unless the user wants another account
    if let Some(token) = putio_token(base) {
        if let Ok((token, username, used)) = probe(token.to_string()).await {
            if !ask {
                return Ok(Some((token, username, used)));
            }
            match prompter.ask(&format!("Keep the Put.io account {username}? (Y/n)"), "y")? {
                Some(a) if yes(&a) => return Ok(Some((token, username, used))),
                Some(_) => {}
                None => return Ok(None),
            }
        }
    }
    if prompter.interactive() {
        if let Some(app_id) = resolve_app_id(base) {
            say!(
                out,
                "Link this install to Put.io: go to {LINK_URL} and enter the code"
            );
            let link = DeviceLink::with_base_url(&env.putio_base_url);
            let code = link.request_code(&app_id).await?;
            say!(out, "    {}", code.code);
            say!(out, "Waiting for Put.io (Ctrl-C to stop)...");
            out.flush()?;
            let started = Instant::now();
            while started.elapsed() < LINK_TIMEOUT {
                match link.check_code(&code.code).await {
                    Ok(OobCheck::Linked(token)) => return probe(token).await.map(Some),
                    Ok(OobCheck::Pending) => {}
                    Err(putio_sync_client::ClientError::AuthError(_)) => break,
                    Err(e) => say!(out, "  {e}; trying again"),
                }
                tokio::time::sleep(LINK_POLL).await;
            }
            bail!("the link code expired; run setup again");
        }
    }
    let prompt = "Put.io OAuth token (from put.io → Settings → API)";
    loop {
        let Some(token) = prompter.token_line(prompt)? else {
            if prompter.interactive() {
                return Ok(None);
            }
            bail!("no Put.io token: set PUTIO_SYNC_PUTIO_TOKEN or pass it on standard input");
        };
        if token.is_empty() {
            if !prompter.interactive() {
                bail!("no Put.io token: set PUTIO_SYNC_PUTIO_TOKEN or pass it on standard input");
            }
            continue;
        }
        match probe(token).await {
            Ok(found) => return Ok(Some(found)),
            Err(e) if prompter.interactive() => say!(out, "  {e}"),
            Err(e) => return Err(e),
        }
    }
}

fn yes(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn describe_warning(warning: &FolderWarning) -> String {
    match warning {
        FolderWarning::LessThanPutioUsed { free, used } => format!(
            "{} free here, less than the {} your Put.io account uses",
            human_bytes(*free),
            human_bytes(*used)
        ),
        FolderWarning::LowSpace { free } => format!("only {} free", human_bytes(*free)),
        FolderWarning::ContainerPath => {
            "this is a path inside the container; map it to a folder on the host".to_string()
        }
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    const ACCOUNT: &str = r#"{"status":"OK","info":{"user_id":1,"username":"alice","mail":"alice@example.com","account_active":true,"disk":{"size":1000000,"used":5000,"avail":995000},"avatar_url":null}}"#;
    const ROOT: &str = r#"{"status":"OK","parent":{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false},"files":[{"id":5,"name":"film.mkv","size":4000,"file_type":"VIDEO","parent_id":0,"is_folder":false,"is_shared":false}]}"#;

    struct Script {
        interactive: bool,
        answers: VecDeque<String>,
        asked: Vec<String>,
    }

    impl Script {
        fn new(interactive: bool, answers: &[&str]) -> Self {
            Self {
                interactive,
                answers: answers.iter().map(|a| a.to_string()).collect(),
                asked: Vec::new(),
            }
        }
    }

    impl Prompter for Script {
        fn interactive(&self) -> bool {
            self.interactive
        }
        fn ask(&mut self, question: &str, default: &str) -> Result<Option<String>> {
            self.asked.push(question.to_string());
            Ok(self
                .answers
                .pop_front()
                .map(|a| if a.is_empty() { default.to_string() } else { a }))
        }
        fn token_line(&mut self, prompt: &str) -> Result<Option<String>> {
            self.asked.push(prompt.to_string());
            Ok(self.answers.pop_front())
        }
    }

    async fn putio() -> mockito::ServerGuard {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/account/info")
            .match_header("authorization", "Bearer good")
            .with_body(ACCOUNT)
            .create_async()
            .await;
        server
            .mock("GET", "/account/info")
            .match_header("authorization", "Bearer bad")
            .with_status(401)
            .with_body(r#"{"status":"ERROR","error_type":"invalid_grant"}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/files/list?parent_id=0")
            .with_body(ROOT)
            .create_async()
            .await;
        server
    }

    fn env(server: &mockito::ServerGuard, token: Option<&str>) -> SetupEnv {
        SetupEnv {
            putio_token: token.map(str::to_string),
            api_token_in_env: false,
            putio_base_url: server.url(),
        }
    }

    #[tokio::test]
    async fn an_interactive_run_writes_the_config_once() {
        let server = putio().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let downloads = dir.path().join("downloads");
        // bad token, good token, folder, preset (default), watch (none), metrics, write
        let mut script = Script::new(
            true,
            &[
                "bad",
                "good",
                &downloads.display().to_string(),
                "",
                "",
                "n",
                "y",
            ],
        );
        let mut out = Vec::new();
        let finished = run_setup(
            &path,
            &SetupArgs::default(),
            &env(&server, None),
            &mut script,
            &mut out,
        )
        .await
        .unwrap();
        assert_eq!(finished, Finished::Written);
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("Put.io refused the token"), "{out}");
        assert!(out.contains("Connected to Put.io as alice"));
        assert!(
            out.contains("1 files, 4.0 KB, would be downloaded"),
            "{out}"
        );
        assert!(out.contains("film.mkv"));

        let config = Config::load(&path).unwrap();
        assert_eq!(config.download.target_directory, downloads);
        assert!(downloads.is_dir());
        assert!(!config.watch.enabled);
        let api_token = config.api.auth_token.clone().unwrap();
        assert_eq!(api_token.len(), 64);
        assert_eq!(
            out.matches(&api_token).count(),
            1,
            "the API token is printed once"
        );
        assert!(!out.contains("good"), "the Put.io token is never printed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[tokio::test]
    async fn saying_no_writes_nothing() {
        let server = putio().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut script = Script::new(true, &["good", "", "", "", "n"]);
        let args = SetupArgs {
            download_dir: Some(dir.path().to_path_buf()),
            no_dry_run: true,
            ..SetupArgs::default()
        };
        let mut out = Vec::new();
        let finished = run_setup(&path, &args, &env(&server, None), &mut script, &mut out)
            .await
            .unwrap();
        assert_eq!(finished, Finished::Cancelled);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn yes_takes_flags_and_suggestions_without_asking() {
        let server = putio().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let args = SetupArgs {
            download_dir: Some(dir.path().join("dl")),
            preset: Some("video".to_string()),
            metrics: Some(true),
            no_dry_run: true,
            yes: true,
            ..SetupArgs::default()
        };
        let mut script = Script::new(false, &[]);
        let mut out = Vec::new();
        let finished = run_setup(
            &path,
            &args,
            &env(&server, Some("good")),
            &mut script,
            &mut out,
        )
        .await
        .unwrap();
        assert_eq!(finished, Finished::Written);
        assert!(script.asked.is_empty(), "{:?}", script.asked);
        let config = Config::load(&path).unwrap();
        assert_eq!(config.download.preset.as_deref(), Some("video"));
        assert!(config.metrics.enabled);
    }

    #[tokio::test]
    async fn an_unusable_folder_fails_with_yes_and_names_the_flag() {
        let server = putio().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let file = dir.path().join("a-file");
        std::fs::write(&file, b"x").unwrap();
        let args = SetupArgs {
            download_dir: Some(file),
            yes: true,
            ..SetupArgs::default()
        };
        let err = run_setup(
            &path,
            &args,
            &env(&server, Some("good")),
            &mut Script::new(false, &[]),
            &mut Vec::new(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("--download-dir"), "{err}");
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn without_a_terminal_the_token_comes_from_stdin() {
        let server = putio().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let args = SetupArgs {
            download_dir: Some(dir.path().to_path_buf()),
            no_dry_run: true,
            ..SetupArgs::default()
        };
        let mut script = Script::new(false, &["good"]);
        let finished = run_setup(
            &path,
            &args,
            &env(&server, None),
            &mut script,
            &mut Vec::new(),
        )
        .await
        .unwrap();
        assert_eq!(finished, Finished::Written);

        let mut empty = Script::new(false, &[]);
        let other = dir.path().join("other.toml");
        let err = run_setup(
            &other,
            &args,
            &env(&server, None),
            &mut empty,
            &mut Vec::new(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("PUTIO_SYNC_PUTIO_TOKEN"), "{err}");
    }

    #[tokio::test]
    async fn an_unknown_preset_is_an_error() {
        let server = putio().await;
        let dir = tempfile::tempdir().unwrap();
        let args = SetupArgs {
            download_dir: Some(dir.path().to_path_buf()),
            preset: Some("nope".to_string()),
            yes: true,
            ..SetupArgs::default()
        };
        let err = run_setup(
            &dir.path().join("c.toml"),
            &args,
            &env(&server, Some("good")),
            &mut Script::new(false, &[]),
            &mut Vec::new(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("--preset"), "{err}");
    }
}
