//! `languages`: the languages this build can show, and the one in use (feature 060)

use crate::output::pad;
use putio_sync_core::i18n::{self, Source};
use putio_sync_core::t;
use std::io::{self, Write};

/// Print the list. `--json` is the same in every language.
pub fn execute(json: bool, out: &mut impl Write) -> io::Result<()> {
    let choice = i18n::current();
    let languages = crate::i18n::available();
    if json {
        let value = serde_json::json!({
            "current": { "code": choice.lang.code, "source": choice.source.as_str() },
            "languages": languages
                .iter()
                .map(|l| serde_json::json!({ "code": l.code, "name": l.name, "rtl": l.rtl }))
                .collect::<Vec<_>>(),
        });
        return writeln!(out, "{value}");
    }
    writeln!(out, "{} {}", pad(&t!("CODE"), 6), t!("LANGUAGE"))?;
    for lang in &languages {
        writeln!(out, "{} {}", pad(lang.code, 6), lang.name)?;
    }
    let from = match choice.source {
        Source::Flag => "--lang".to_string(),
        Source::Env => "PUTIO_SYNC_LANG".to_string(),
        Source::Config => t!("config"),
        Source::Os => choice
            .os_variable
            .map(str::to_string)
            .unwrap_or_else(|| t!("system")),
        Source::Default => t!("default"),
    };
    writeln!(out)?;
    writeln!(
        out,
        "{}",
        t!("In use: {0} (from {1})", choice.lang.code, from)
    )
}
