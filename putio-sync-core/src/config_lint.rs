//! Config file warnings for the doctor (feature 010)
//!
//! Loading a config ignores keys it does not know, so a typo such as `skip_exisiting`
//! silently falls back to the default. [`lint`] lists those keys, plus settings that are
//! valid but usually a mistake.

use crate::config::Config;
use serde::Serialize;

/// One finding in a config file
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigWarning {
    /// Dotted key path, e.g. `download.skip_exisiting`
    pub key: String,
    /// What is wrong, in plain words
    pub message: String,
}

/// Warnings for the config file text `contents`
///
/// Fails only when the text is not a valid config at all (the daemon would not start).
pub fn lint(contents: &str) -> Result<Vec<ConfigWarning>, toml::de::Error> {
    let mut unknown = Vec::new();
    let deserializer = toml::Deserializer::new(contents);
    let _: Config = serde_ignored::deserialize(deserializer, |path| {
        unknown.push(path.to_string());
    })?;

    let mut warnings: Vec<ConfigWarning> = unknown
        .into_iter()
        .map(|key| ConfigWarning {
            message: format!("Unknown key `{key}` is ignored; check the spelling"),
            key,
        })
        .collect();

    let raw: toml::Table = contents.parse()?;
    let skip_existing = raw
        .get("download")
        .and_then(|d| d.get("skip_existing"))
        .and_then(toml::Value::as_bool);
    if skip_existing == Some(false) {
        warnings.push(ConfigWarning {
            key: "download.skip_existing".to_string(),
            message: "`skip_existing = false` downloads files again even when they are already \
                      on disk; remove the line unless that is what you want"
                .to_string(),
        });
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default config as TOML, with `edit` applied to its table
    fn config_text(edit: impl FnOnce(&mut toml::Table)) -> String {
        let mut table: toml::Table = toml::from_str(&toml::to_string(&Config::default()).unwrap())
            .expect("default config round-trips");
        edit(&mut table);
        toml::to_string(&table).unwrap()
    }

    fn download(table: &mut toml::Table) -> &mut toml::Table {
        table
            .get_mut("download")
            .and_then(toml::Value::as_table_mut)
            .expect("[download] section")
    }

    fn keys(contents: &str) -> Vec<String> {
        lint(contents).unwrap().into_iter().map(|w| w.key).collect()
    }

    #[test]
    fn clean_config_has_no_warnings() {
        assert!(keys(&config_text(|_| {})).is_empty());
    }

    #[test]
    fn misspelt_top_level_section_is_reported() {
        let text = config_text(|t| {
            t.insert("dowload".into(), toml::Value::Table(toml::Table::new()));
        });
        assert_eq!(keys(&text), vec!["dowload"]);
    }

    #[test]
    fn misspelt_nested_key_is_reported_with_its_path() {
        let text = config_text(|t| {
            download(t).insert("skip_exisiting".into(), true.into());
        });
        assert_eq!(keys(&text), vec!["download.skip_exisiting"]);
    }

    #[test]
    fn explicit_skip_existing_false_is_reported() {
        let text = config_text(|t| {
            download(t).insert("skip_existing".into(), false.into());
        });
        let warnings = lint(&text).unwrap();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].key, "download.skip_existing");
        assert!(warnings[0].message.contains("downloads files again"));
    }

    #[test]
    fn skip_existing_true_is_fine() {
        let text = config_text(|t| {
            download(t).insert("skip_existing".into(), true.into());
        });
        assert!(keys(&text).is_empty());
    }

    #[test]
    fn invalid_toml_is_an_error() {
        assert!(lint("[download\n").is_err());
    }
}
