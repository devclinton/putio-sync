//! The chat prompt and the reply format (research R3, R14).
//!
//! A cue travels as one line `N| text` where `N` is its 1-based position in the whole
//! subtitle and line breaks inside the cue are written `<br>`. The reply is parsed leniently
//! (commentary, code fences, `<think>` blocks and models that break lines on their own are
//! all handled) and then validated strictly: exactly the expected numbers, in order, none
//! empty.

/// One line as sent or received: the cue's 1-based number and its text, line breaks as `\n`
pub type Numbered = (usize, String);

/// Why a reply was not usable
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplyError {
    /// The reply has another number of lines than were sent
    #[error("reply had {got} lines, expected {expected}")]
    WrongLineCount {
        /// Lines asked for
        expected: usize,
        /// Lines found
        got: usize,
    },
    /// A line has the wrong number or the lines are out of order
    #[error("reply line {at} has number {found}, expected {expected}")]
    BadNumbering {
        /// 1-based position in the reply
        at: usize,
        /// Number found
        found: usize,
        /// Number expected
        expected: usize,
    },
    /// A line came back with no text
    #[error("line {number} came back empty")]
    EmptyText {
        /// Cue number
        number: usize,
    },
    /// The model stopped at its length limit
    #[error("the reply was cut off at the model's length limit")]
    Truncated,
    /// No content at all
    #[error("the reply had no content")]
    NoContent,
}

/// Whether a cue has anything to translate: some letter once tags are ignored. `♪`, `...`
/// and `[ ]` are copied as they are and never sent.
pub fn is_translatable(text: &str) -> bool {
    strip_tags(text).chars().any(char::is_alphabetic)
}

/// `text` with `<...>` and `{\...}` tags removed
pub fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if let Some(len) = tag_at(&text[i..]) {
            i += len;
        } else {
            let c = text[i..].chars().next().unwrap_or(' ');
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// Byte length of the tag that starts `s`, if one does
fn tag_at(s: &str) -> Option<usize> {
    let mut chars = s.chars();
    match chars.next()? {
        '<' => {
            let next = chars.next()?;
            if !(next.is_ascii_alphabetic() || next == '/') {
                return None;
            }
            let end = s.find('>')?;
            // a tag does not span a line break
            if s[..end].contains('\n') {
                return None;
            }
            Some(end + 1)
        }
        '{' => {
            if chars.next()? != '\\' {
                return None;
            }
            let end = s.find('}')?;
            if s[..end].contains('\n') {
                return None;
            }
            Some(end + 1)
        }
        _ => None,
    }
}

/// Bytes of the run of tags at the start of `s`
fn leading_tags(s: &str) -> usize {
    let mut at = 0;
    while let Some(len) = tag_at(&s[at..]) {
        at += len;
    }
    at
}

/// Bytes of the run of tags at the end of `s`
fn trailing_tags(s: &str) -> usize {
    let mut end = s.len();
    loop {
        let head = &s[..end];
        let open = if head.ends_with('>') {
            head.rfind('<')
        } else if head.ends_with('}') {
            head.rfind('{')
        } else {
            None
        };
        match open {
            Some(start) if tag_at(&head[start..]) == Some(head.len() - start) => end = start,
            _ => break,
        }
    }
    s.len() - end
}

/// Put back the tags that wrapped the whole source cue (`<i>..</i>`, `{\an8}`) when the
/// model dropped them. Tags in the middle of a cue are left as the model wrote them.
pub fn restore_wrapping(source: &str, output: &str) -> String {
    let lead = &source[..leading_tags(source)];
    let trail_len = trailing_tags(source);
    let trail = &source[source.len() - trail_len..];
    // a source that is only tags has nothing to wrap
    if lead.len() + trail.len() > source.len() {
        return output.to_string();
    }
    let mut out = output.to_string();
    if !lead.is_empty() && !out.starts_with(lead) {
        out = format!("{lead}{out}");
    }
    if !trail.is_empty() && !out.ends_with(trail) {
        out.push_str(trail);
    }
    out
}

/// A cue's text as one line: line breaks become `<br>`
pub fn encode_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "<br>")
}

/// `N| text`
pub fn encode_line(number: usize, text: &str) -> String {
    format!("{number}| {}", encode_text(text))
}

/// The system prompt (research R3). `instruction` is the operator's extra text, may be empty.
pub fn system_prompt(
    source_name: &str,
    source_code: &str,
    target_name: &str,
    target_code: &str,
    instruction: &str,
) -> String {
    let instruction = instruction.trim();
    let extra = if instruction.is_empty() {
        String::new()
    } else {
        format!("\nAdditional instruction from the operator: {instruction}")
    };
    format!(
        "You translate subtitles from {source_name} ({source_code}) to {target_name} ({target_code}).\n\
Each line you receive looks like \"N| text\", where N is the line number.\n\
Reply with one line \"N| translation\" for every line you are asked to translate, in the same order, with the same numbers. \
Output nothing else: no explanations, no code fences, no blank lines.\n\
Never merge, split, drop or add lines. Never change a number.\n\
\"<br>\" marks a line break inside a subtitle. Keep it where the break belongs in your translation.\n\
Keep tags such as <i>, </i>, <b> and {{\\an8}} exactly as they are, around the same words.\n\
Keep names, numbers, music symbols and sound-effect brackets; translate the words inside brackets.\n\
Translate for natural reading as a subtitle: short, spoken style. Do not censor.{extra}"
    )
}

/// The user message: up to three source lines of context, then the lines to translate
pub fn user_prompt(context: &[Numbered], lines: &[Numbered]) -> String {
    let mut out = String::new();
    if !context.is_empty() {
        out.push_str("Context, for reference only. Do not translate or output these lines:\n");
        for (n, text) in context {
            out.push_str(&encode_line(*n, text));
            out.push('\n');
        }
        out.push('\n');
        out.push_str("Translate these lines:\n");
    }
    for (i, (n, text)) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&encode_line(*n, text));
    }
    out
}

/// Remove `<think>...</think>` blocks (and everything before a lone `</think>`)
pub fn strip_think(text: &str) -> String {
    let mut out = text.to_string();
    while let Some(start) = out.find("<think>") {
        match out[start..].find("</think>") {
            Some(rel) => out.replace_range(start..start + rel + "</think>".len(), ""),
            None => {
                // never closed: the rest is reasoning
                out.truncate(start);
                break;
            }
        }
    }
    if let Some(close) = out.rfind("</think>") {
        out = out[close + "</think>".len()..].to_string();
    }
    out
}

/// Turn `<br>`, `<br/>`, `<BR />` into line breaks
pub fn decode_breaks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        if let Some(end) = tail.find('>') {
            let inner = tail[1..end].trim().trim_end_matches('/').trim();
            if inner.eq_ignore_ascii_case("br") {
                out.push('\n');
                rest = &tail[end + 1..];
                continue;
            }
        }
        out.push('<');
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// A line `N| text` (also `N｜`): the number and the text after the bar
fn parse_line(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_start();
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let number: usize = t[..digits].parse().ok()?;
    let rest = t[digits..].trim_start();
    let rest = rest
        .strip_prefix('|')
        .or_else(|| rest.strip_prefix('\u{ff5c}'))?;
    Some((number, rest.strip_prefix(' ').unwrap_or(rest)))
}

/// Parse a reply into numbered lines. Think blocks and code fences are removed, lines before
/// the first and after the last numbered line are commentary, and a line that does not look
/// numbered between two numbered ones continues the previous cue.
pub fn parse_reply(raw: &str) -> Vec<Numbered> {
    let text = strip_think(raw);
    let mut out: Vec<Numbered> = Vec::new();
    // continuation lines wait here until a later numbered line proves they were not trailing
    // commentary
    let mut pending: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            continue;
        }
        if let Some((n, body)) = parse_line(line) {
            if let Some(last) = out.last_mut() {
                for extra in pending.drain(..) {
                    last.1.push_str("<br>");
                    last.1.push_str(&extra);
                }
            }
            pending.clear();
            out.push((n, body.trim_end().to_string()));
        } else if !trimmed.is_empty() && !out.is_empty() {
            pending.push(trimmed.to_string());
        }
    }
    out.into_iter()
        .map(|(n, t)| (n, decode_breaks(&t).trim().to_string()))
        .collect()
}

/// Check a parsed reply against what was asked and return the texts in `expected` order.
/// Lines that echo a `context` number are dropped first.
pub fn validate(
    expected: &[usize],
    context: &[usize],
    mut parsed: Vec<Numbered>,
) -> Result<Vec<String>, ReplyError> {
    if parsed.is_empty() {
        return Err(ReplyError::NoContent);
    }
    parsed.retain(|(n, _)| !context.contains(n) || expected.contains(n));
    if parsed.len() != expected.len() {
        return Err(ReplyError::WrongLineCount {
            expected: expected.len(),
            got: parsed.len(),
        });
    }
    for (i, ((found, text), want)) in parsed.iter().zip(expected).enumerate() {
        if found != want {
            return Err(ReplyError::BadNumbering {
                at: i + 1,
                found: *found,
                expected: *want,
            });
        }
        if text.trim().is_empty() {
            return Err(ReplyError::EmptyText { number: *found });
        }
    }
    Ok(parsed.into_iter().map(|(_, t)| t).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(items: &[(usize, &str)]) -> Vec<Numbered> {
        items.iter().map(|(n, t)| (*n, t.to_string())).collect()
    }

    #[test]
    fn a_cue_is_one_numbered_line_with_br_for_breaks() {
        assert_eq!(
            encode_line(7, "Hello.\nHow are you?"),
            "7| Hello.<br>How are you?"
        );
        assert_eq!(encode_line(8, "Plain"), "8| Plain");
        assert_eq!(encode_text("a\r\nb"), "a<br>b");
    }

    #[test]
    fn system_prompt_names_languages_and_adds_the_instruction_only_when_set() {
        let p = system_prompt("English", "en", "Spanish", "es", "");
        assert!(p.starts_with("You translate subtitles from English (en) to Spanish (es)."));
        assert!(p.contains("\"N| text\""));
        assert!(p.contains("Never merge, split, drop or add lines."));
        assert!(p.contains("{\\an8}"));
        assert!(!p.contains("Additional instruction"));
        assert!(!p.ends_with('\n'));
        let p = system_prompt(
            "English",
            "en",
            "Spanish",
            "es",
            " informal Spanish (Spain) ",
        );
        assert!(p.ends_with("Additional instruction from the operator: informal Spanish (Spain)"));
    }

    #[test]
    fn user_prompt_has_context_only_when_given() {
        let p = user_prompt(&[], &lines(&[(1, "Hi"), (2, "a\nb")]));
        assert_eq!(p, "1| Hi\n2| a<br>b");
        let p = user_prompt(&lines(&[(1, "Hi"), (2, "Yo")]), &lines(&[(3, "There")]));
        assert_eq!(
            p,
            "Context, for reference only. Do not translate or output these lines:\n1| Hi\n2| Yo\n\nTranslate these lines:\n3| There"
        );
    }

    #[test]
    fn replies_parse_in_the_plain_shape() {
        let got = parse_reply("1| Hola.\n2| Adios<br>amigo");
        assert_eq!(got, lines(&[(1, "Hola."), (2, "Adios\namigo")]));
    }

    #[test]
    fn break_spellings_all_become_newlines() {
        let got = parse_reply("1| a<br/>b<BR />c<br >d");
        assert_eq!(got, lines(&[(1, "a\nb\nc\nd")]));
        // a lone angle bracket is kept
        assert_eq!(parse_reply("1| 2 < 3 and <3")[0].1, "2 < 3 and <3");
    }

    #[test]
    fn think_blocks_fences_and_commentary_are_dropped() {
        let raw = "<think>I should\nkeep numbers\n1| decoy</think>\nHere is the translation:\n```\n1| Hola\n2| Adios\n```\nHope this helps!";
        assert_eq!(parse_reply(raw), lines(&[(1, "Hola"), (2, "Adios")]));
        // only a closing tag
        assert_eq!(
            parse_reply("reasoning</think>\n1| Hola"),
            lines(&[(1, "Hola")])
        );
        // never closed
        assert!(parse_reply("<think>1| nope").is_empty());
    }

    #[test]
    fn spacing_and_the_wide_bar_are_accepted() {
        let got = parse_reply("  1 | Hola\n2｜Adios\n3|   spaced");
        assert_eq!(got, lines(&[(1, "Hola"), (2, "Adios"), (3, "spaced")]));
    }

    #[test]
    fn a_stray_line_between_numbered_lines_continues_the_cue() {
        let got = parse_reply("1| first part\nsecond part\n\n2| next\n3| last");
        assert_eq!(
            got,
            lines(&[(1, "first part\nsecond part"), (2, "next"), (3, "last")])
        );
    }

    #[test]
    fn validation_wants_exactly_the_expected_numbers_in_order() {
        let ok = validate(&[4, 5], &[], lines(&[(4, "a"), (5, "b")])).unwrap();
        assert_eq!(ok, vec!["a", "b"]);
        assert_eq!(
            validate(&[4, 5], &[], lines(&[(4, "a")])),
            Err(ReplyError::WrongLineCount {
                expected: 2,
                got: 1
            })
        );
        assert_eq!(
            validate(&[4, 5], &[], lines(&[(4, "a"), (5, "b"), (6, "c")])),
            Err(ReplyError::WrongLineCount {
                expected: 2,
                got: 3
            })
        );
        assert!(matches!(
            validate(&[4, 5], &[], lines(&[(5, "b"), (4, "a")])),
            Err(ReplyError::BadNumbering { at: 1, .. })
        ));
        assert!(matches!(
            validate(&[4, 5], &[], lines(&[(4, "a"), (4, "a")])),
            Err(ReplyError::BadNumbering { at: 2, .. })
        ));
        assert_eq!(
            validate(&[4, 5], &[], lines(&[(4, "a"), (5, " ")])),
            Err(ReplyError::EmptyText { number: 5 })
        );
        assert_eq!(validate(&[1], &[], vec![]), Err(ReplyError::NoContent));
    }

    #[test]
    fn echoed_context_lines_are_trimmed_first() {
        let got = validate(&[4], &[2, 3], lines(&[(2, "x"), (3, "y"), (4, "z")])).unwrap();
        assert_eq!(got, vec!["z"]);
        // a number outside both is still wrong
        assert!(validate(&[4], &[2, 3], lines(&[(9, "z")])).is_err());
    }

    #[test]
    fn whole_cue_tags_are_restored_when_dropped() {
        assert_eq!(restore_wrapping("<i>Hello</i>", "Hola"), "<i>Hola</i>");
        assert_eq!(
            restore_wrapping("<i>Hello</i>", "<i>Hola</i>"),
            "<i>Hola</i>"
        );
        assert_eq!(restore_wrapping("<i>Hello</i>", "Hola</i>"), "<i>Hola</i>");
        assert_eq!(restore_wrapping("{\\an8}Top", "Arriba"), "{\\an8}Arriba");
        assert_eq!(
            restore_wrapping("{\\an8}<i>Top</i>", "Arriba"),
            "{\\an8}<i>Arriba</i>"
        );
        assert_eq!(
            restore_wrapping("<i>one\ntwo</i>", "uno\ndos"),
            "<i>uno\ndos</i>"
        );
        // tags inside a cue are left to the model
        assert_eq!(
            restore_wrapping("Hello <i>you</i> there", "Hola tu alli"),
            "Hola tu alli"
        );
        assert_eq!(restore_wrapping("Plain", "Llano"), "Llano");
        assert_eq!(
            restore_wrapping("<font color=\"red\">Hi</font>", "Hola"),
            "<font color=\"red\">Hola</font>"
        );
    }

    #[test]
    fn cues_with_no_letter_are_not_translatable() {
        for text in [
            "♪",
            "♪ ♪",
            "...",
            "[ ]",
            "<i>♪</i>",
            "{\\an8}...",
            "12:30",
            "- 42",
        ] {
            assert!(!is_translatable(text), "{text}");
        }
        for text in [
            "Hi",
            "<i>Hi</i>",
            "[door slams]",
            "日本語",
            "Привет",
            "♪ la la ♪",
        ] {
            assert!(is_translatable(text), "{text}");
        }
    }

    #[test]
    fn tags_strip_and_survive_odd_input() {
        assert_eq!(strip_tags("<i>a</i> {\\an8}b <3 c"), "a b <3 c");
        assert_eq!(strip_tags(""), "");
        assert_eq!(strip_tags("{not a tag}"), "{not a tag}");
    }
}
