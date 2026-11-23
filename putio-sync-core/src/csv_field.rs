//! One CSV field, quoted and defused against spreadsheet formula injection

/// A CSV field, quoted when it holds a comma, quote or line break. A leading `=`, `+`, `-`,
/// `@`, tab or carriage return gets a `'` so a spreadsheet doesn't run a file or release
/// name as a formula.
pub fn csv_field(value: &str) -> String {
    let value = match value.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r') => format!("'{value}"),
        _ => value.to_string(),
    };
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_quoted() {
        assert_eq!(csv_field("Movie.2024"), "Movie.2024");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("line\nbreak"), "\"line\nbreak\"");
    }

    #[test]
    fn formula_starts_are_defused() {
        for lead in ["=", "+", "-", "@", "\t"] {
            let v = format!("{lead}HYPERLINK(\"x\")");
            assert!(
                csv_field(&v).trim_start_matches('"').starts_with('\''),
                "{lead:?}"
            );
        }
        assert_eq!(csv_field("=cmd()"), "'=cmd()");
        assert_eq!(csv_field("\r=x"), "\"'\r=x\"");
    }
}
