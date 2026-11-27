//! One `[y/N]` prompt for every command (feature 060)

use putio_sync_core::i18n;
use std::io::{self, BufRead, Write};

/// The words that mean yes in the run's language, lower-case: the catalog's `"y|yes"`
/// entry. English `y` and `yes` are always accepted too.
fn yes_words() -> Vec<String> {
    i18n::tr("y|yes")
        .split('|')
        .map(|word| word.trim().to_lowercase())
        .filter(|word| !word.is_empty())
        .collect()
}

/// Whether `answer` is a yes: `y`, `yes` or one of `words`, in any case. Anything else,
/// an empty line included, is no.
pub fn is_yes(answer: &str, words: &[String]) -> bool {
    let answer = answer.trim().to_lowercase();
    answer == "y" || answer == "yes" || words.contains(&answer)
}

/// `question [y/N] `, with the language's own letter for yes
pub fn prompt(question: &str) -> String {
    let words = yes_words();
    let yes = words.first().map(String::as_str).unwrap_or("y");
    format!("{question} [{yes}/N] ")
}

/// Ask on `out` and read the answer from `input`
pub fn confirm_with(
    question: &str,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> io::Result<bool> {
    write!(out, "{}", prompt(question))?;
    out.flush()?;
    let mut answer = String::new();
    input.read_line(&mut answer)?;
    Ok(is_yes(&answer, &yes_words()))
}

/// Ask on standard output and read the answer from standard input. The question is
/// already in the run's language.
pub fn confirm(question: &str) -> io::Result<bool> {
    confirm_with(question, &mut io::stdin().lock(), &mut io::stdout())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yes_is_y_yes_or_the_language_words() {
        let words = vec!["j".to_string(), "ja".to_string()];
        for yes in ["y", "Y", "yes", " YES \n", "j", "Ja\n"] {
            assert!(is_yes(yes, &words), "{yes:?}");
        }
        for no in ["", "\n", "n", "no", "nein", "yy", "oui"] {
            assert!(!is_yes(no, &words), "{no:?}");
        }
    }

    #[test]
    fn the_prompt_is_printed_and_the_answer_read() {
        let mut out = Vec::new();
        assert!(confirm_with("Delete 2 files?", &mut "yes\n".as_bytes(), &mut out).unwrap());
        assert_eq!(String::from_utf8(out).unwrap(), "Delete 2 files? [y/N] ");
        assert!(!confirm_with("Delete?", &mut "".as_bytes(), &mut Vec::new()).unwrap());
    }
}
