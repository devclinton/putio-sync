//! The CLI's side of the language choice (feature 060): the embedded catalogs, translated
//! help, and translated messages for the common argument errors. The choice itself, `t!`
//! and `msg!` are in `putio_sync_core::i18n`.

use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{Arg, ArgAction, Command};
use putio_sync_core::i18n::{self as core, Lang, Source, EN};
use putio_sync_core::t;

include!(concat!(env!("OUT_DIR"), "/i18n_catalogs.rs"));

/// The languages this build can show: English, then every embedded catalog
pub fn available() -> Vec<Lang> {
    core::LANGS
        .iter()
        .copied()
        .filter(|lang| *lang == EN || CATALOGS.iter().any(|(code, _)| *code == lang.code))
        .collect()
}

/// The chosen language's catalog: English text → translation. The build packs the English
/// keys once and each language as the list of its values in the same order.
fn catalog(lang: Lang) -> Option<std::collections::HashMap<String, String>> {
    fn unpack<T: serde::de::DeserializeOwned>(packed: &[u8]) -> Option<T> {
        use std::io::Read;
        let mut text = String::new();
        flate2::read::DeflateDecoder::new(packed)
            .read_to_string(&mut text)
            .ok()?;
        serde_json::from_str(&text).ok()
    }
    let (_, packed) = CATALOGS.iter().find(|(code, _)| *code == lang.code)?;
    let keys: Vec<String> = unpack(KEYS)?;
    let values: Vec<Option<String>> = unpack(packed)?;
    Some(
        keys.into_iter()
            .zip(values)
            .filter_map(|(key, value)| Some((key, value?)))
            .collect(),
    )
}

/// Pick the run's language and load its catalog. Call this first in `main`, before the
/// arguments are parsed. Prints at most one line per bad value on standard error (not with
/// `-q`), and never changes the exit code.
pub fn init() {
    let args: Vec<String> = std::env::args_os()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let env = |name: &str| std::env::var(name).ok();
    let mut choice = core::select(
        &args,
        &env,
        &|| core::config_language(&args, &env),
        &core::os_locales,
    );
    if choice.lang != EN {
        match catalog(choice.lang) {
            Some(entries) => core::set_catalog(entries),
            None => {
                // A build without the catalogs. Say so only when the language was asked
                // for by name; a system locale falls back quietly.
                choice.english_only = choice.source != Source::Os;
                choice.lang = EN;
                choice.source = Source::Default;
                choice.os_variable = None;
            }
        }
    }
    let quiet = args.iter().any(|arg| arg == "-q" || arg == "--quiet");
    core::set_choice(choice);
    if quiet {
        return;
    }
    let choice = core::current();
    let codes: Vec<&str> = available().iter().map(|lang| lang.code).collect();
    for value in &choice.warnings {
        eprintln!(
            "putio-sync: {}",
            t!(
                "language \"{0}\" is not available; using {1}. Available: {2} (see `putio-sync languages`). Use --lang en for English.",
                value,
                choice.lang.code,
                codes.join(", ")
            )
        );
    }
    if choice.english_only {
        eprintln!("putio-sync: {}", t!("this build has English only."));
    }
}

/// Fixed help text that is not in the command tree; the catalog test collects it from here
pub const FIXED_KEYS: &[&str] = &[
    "y|yes",
    "Usage:",
    "Commands:",
    "Arguments:",
    "Options:",
    "Print help",
    "Print version",
];

/// `cmd` with its help in the run's language: descriptions, argument help, headings and
/// the built-in `--help`/`--version`. Names of commands, flags and values are unchanged.
/// In English the command comes back as it is.
pub fn localize(cmd: Command) -> Command {
    if core::current().lang == EN {
        return cmd;
    }
    translate_command(cmd)
}

fn translate_command(mut cmd: Command) -> Command {
    let text = |s: &clap::builder::StyledStr| core::tr(&s.to_string()).to_string();
    if let Some(about) = cmd.get_about().map(text) {
        cmd = cmd.about(about);
    }
    if let Some(about) = cmd.get_long_about().map(text) {
        cmd = cmd.long_about(about);
    }
    if let Some(help) = cmd.get_after_help().map(text) {
        cmd = cmd.after_help(help);
    }
    if let Some(help) = cmd.get_before_help().map(text) {
        cmd = cmd.before_help(help);
    }
    cmd = cmd.mut_args(|mut arg| {
        if let Some(help) = arg.get_help().map(text) {
            arg = arg.help(help);
        }
        if let Some(help) = arg.get_long_help().map(text) {
            arg = arg.long_help(help);
        }
        arg
    });

    // The built-in flags, with translated help
    let has_version = cmd.get_version().is_some() || cmd.get_long_version().is_some();
    cmd = cmd.disable_help_flag(true).arg(
        Arg::new("help")
            .short('h')
            .long("help")
            .action(ArgAction::Help)
            .help(core::tr("Print help").to_string()),
    );
    if has_version {
        cmd = cmd.disable_version_flag(true).arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .action(ArgAction::Version)
                .help(core::tr("Print version").to_string()),
        );
    }

    let has_commands = cmd.get_subcommands().any(|sub| !sub.is_hide_set());
    let has_positionals = cmd.get_positionals().any(|arg| !arg.is_hide_set());
    let mut template = format!(
        "{{before-help}}{{about-with-newline}}\n{} {{usage}}\n",
        core::tr("Usage:")
    );
    if has_commands {
        template.push_str(&format!("\n{}\n{{subcommands}}\n", core::tr("Commands:")));
    }
    if has_positionals {
        template.push_str(&format!("\n{}\n{{positionals}}\n", core::tr("Arguments:")));
    }
    template.push_str(&format!(
        "\n{}\n{{options}}{{after-help}}",
        core::tr("Options:")
    ));
    cmd = cmd.help_template(template);

    let names: Vec<String> = cmd
        .get_subcommands()
        .map(|sub| sub.get_name().to_string())
        .collect();
    for name in names {
        cmd = cmd.mut_subcommand(name, translate_command);
    }
    cmd
}

fn strings(err: &clap::Error, kind: ContextKind) -> Vec<String> {
    match err.get(kind) {
        Some(ContextValue::String(s)) => vec![s.clone()],
        Some(ContextValue::Strings(list)) => list.clone(),
        Some(ContextValue::StyledStr(s)) => vec![s.to_string()],
        Some(ContextValue::StyledStrs(list)) => list.iter().map(|s| s.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// The translated message for one of the common argument errors; `None` for the rest, which
/// print as clap writes them
pub fn argument_error(err: &clap::Error) -> Option<String> {
    let one = |kind| strings(err, kind).into_iter().next();
    let mut lines = Vec::new();
    match err.kind() {
        ErrorKind::UnknownArgument => {
            lines.push(t!(
                "unexpected argument '{0}'",
                one(ContextKind::InvalidArg)?
            ));
            if let Some(similar) = one(ContextKind::SuggestedArg) {
                lines.push(t!("a similar argument exists: '{0}'", similar));
            }
        }
        ErrorKind::InvalidValue => {
            lines.push(t!(
                "invalid value '{0}' for '{1}'",
                one(ContextKind::InvalidValue)?,
                one(ContextKind::InvalidArg)?
            ));
            let valid = strings(err, ContextKind::ValidValue);
            if !valid.is_empty() {
                lines.push(t!("possible values: {0}", valid.join(", ")));
            }
        }
        ErrorKind::MissingRequiredArgument => {
            lines.push(t!(
                "the following required arguments were not provided: {0}",
                strings(err, ContextKind::InvalidArg).join(" ")
            ));
        }
        ErrorKind::InvalidSubcommand => {
            lines.push(t!(
                "unrecognized subcommand '{0}'",
                one(ContextKind::InvalidSubcommand)?
            ));
            if let Some(similar) = one(ContextKind::SuggestedSubcommand) {
                lines.push(t!("a similar subcommand exists: '{0}'", similar));
            }
        }
        ErrorKind::NoEquals => {
            lines.push(t!(
                "an equals sign is needed when assigning a value to '{0}'",
                one(ContextKind::InvalidArg)?
            ));
        }
        _ => return None,
    }
    let mut out = t!("error: {0}", lines.remove(0));
    for line in lines {
        out.push_str(&format!("\n  {line}"));
    }
    if let Some(usage) = one(ContextKind::Usage) {
        let usage = usage.trim();
        let usage = usage.strip_prefix("Usage:").unwrap_or(usage).trim();
        out.push_str(&format!("\n\n{} {usage}", core::tr("Usage:")));
    }
    out.push_str(&format!(
        "\n\n{}",
        t!("For more information, try '--help'.")
    ));
    Some(out)
}

/// Print an argument error and exit as clap would (2 for a usage error, 0 for help and
/// version). The common errors are printed in the run's language.
pub fn exit_with(err: clap::Error) -> ! {
    if core::current().lang != EN && err.use_stderr() {
        if let Some(message) = argument_error(&err) {
            eprintln!("{message}");
            std::process::exit(2);
        }
    }
    err.exit()
}
