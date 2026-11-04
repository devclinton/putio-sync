//! Saved file patterns (feature 020): the list filter language of `ui/query.js` (feature
//! 045) in Rust, for folder rules and the account-wide Only / Never download settings.
//!
//! Grammar: `specs/020-glob-filters/contracts/pattern-grammar.md`. Spaces (or `AND`) join
//! terms that must all match; `OR`, `|` or `,` between terms make alternatives and bind
//! tighter; `-` or `NOT` negates a term. `*` and `?` are globs that must match the whole
//! name, a term without them matches anywhere; `{a,b}` means either; a term with `/` is
//! tested against the file's path. Case never matters and brackets are literal.
//!
//! Both matchers run `tests/data/filter-cases.json`, so they can't drift.

use serde::{Serialize, Serializer};

/// Most names one term may stand for after `{…}` expansion
pub const MAX_ALTERNATIVES: usize = 64;

/// Longest pattern text accepted when saved
pub const MAX_LEN: usize = 1000;

/// A parsed pattern; an empty one matches every file
#[derive(Debug, Clone, Default)]
pub struct Pattern {
    text: String,
    groups: Vec<Vec<Term>>,
}

#[derive(Debug, Clone)]
struct Term {
    not: bool,
    /// Tested against the path rather than the name (the text has `/`)
    path: bool,
    /// Each alternative, anchored; a term without globs is wrapped in `*…*`
    alts: Vec<Vec<Glob>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Glob {
    Char(char),
    Any,
    Star,
}

/// A word or operator: `{ text, quoted, neg }` as in `query.js`
#[derive(Debug)]
struct Token {
    text: String,
    quoted: bool,
    /// `-"…"`
    neg: bool,
    /// A quoted token ran to the end without its closing quote
    unclosed: bool,
}

impl Token {
    fn word(&self, w: &str) -> bool {
        !self.quoted && self.text.eq_ignore_ascii_case(w)
    }

    fn is_or(&self) -> bool {
        self.word("OR") || (!self.quoted && self.text.starts_with(['|', ',']))
    }
}

/// The `query.js` tokenizer: quoted (with an optional `-`), runs of `|` / `,`, and bare
/// words in which `{…}` may hold commas
fn tokens(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '"' || (c == '-' && chars.get(i + 1) == Some(&'"')) {
            let neg = c == '-';
            i += if neg { 2 } else { 1 };
            let start = i;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            let unclosed = i == chars.len();
            out.push(Token {
                text: chars[start..i].iter().collect(),
                quoted: true,
                neg,
                unclosed,
            });
            i += 1;
        } else if c == '|' || c == ',' {
            let start = i;
            while i < chars.len() && matches!(chars[i], '|' | ',') {
                i += 1;
            }
            out.push(Token {
                text: chars[start..i].iter().collect(),
                quoted: false,
                neg: false,
                unclosed: false,
            });
        } else {
            let start = i;
            while i < chars.len() {
                let c = chars[i];
                if c == '{' {
                    i += 1;
                    while i < chars.len()
                        && !(chars[i] == '}'
                            || chars[i].is_whitespace()
                            || matches!(chars[i], '"' | '|'))
                    {
                        i += 1;
                    }
                    if chars.get(i) == Some(&'}') {
                        i += 1;
                    }
                } else if c.is_whitespace() || matches!(c, '|' | '"' | ',') {
                    break;
                } else {
                    i += 1;
                }
            }
            out.push(Token {
                text: chars[start..i].iter().collect(),
                quoted: false,
                neg: false,
                unclosed: false,
            });
        }
    }
    out
}

/// One term's text split at `{…}` groups: literal runs and lists of alternatives. A `{`
/// with no `}` before the next `{` is literal, and so is a stray `}`.
fn pieces(text: &str) -> Vec<Vec<String>> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            if let Some(len) = chars[i + 1..]
                .iter()
                .position(|&c| c == '}' || c == '{')
                .filter(|&n| chars[i + 1 + n] == '}')
            {
                if !lit.is_empty() {
                    out.push(vec![std::mem::take(&mut lit)]);
                }
                let inner: String = chars[i + 1..i + 1 + len].iter().collect();
                out.push(inner.split(',').map(str::to_string).collect());
                i += len + 2;
                continue;
            }
        }
        lit.push(chars[i]);
        i += 1;
    }
    if !lit.is_empty() {
        out.push(vec![lit]);
    }
    out
}

/// How many names `text` stands for after `{…}` expansion
fn alternatives(text: &str) -> usize {
    pieces(text)
        .iter()
        .fold(1usize, |n, p| n.saturating_mul(p.len()))
}

/// One char folded for comparison, keeping chars whose lower case is longer
fn fold(c: char) -> char {
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(a), None) => a,
        _ => c,
    }
}

impl Term {
    fn new(text: &str, not: bool) -> Self {
        let glob = text.contains(['*', '?']);
        let mut names = vec![String::new()];
        for piece in pieces(text) {
            names = names
                .iter()
                .flat_map(|n| piece.iter().map(move |p| format!("{n}{p}")))
                .take(MAX_ALTERNATIVES)
                .collect();
        }
        let alts = names
            .iter()
            .map(|n| {
                let body = n.chars().map(|c| match c {
                    '*' => Glob::Star,
                    '?' => Glob::Any,
                    c => Glob::Char(fold(c)),
                });
                if glob {
                    body.collect()
                } else {
                    std::iter::once(Glob::Star)
                        .chain(body)
                        .chain(std::iter::once(Glob::Star))
                        .collect()
                }
            })
            .collect();
        Self {
            not,
            path: text.contains('/'),
            alts,
        }
    }

    fn matches(&self, name: &str, path: &str) -> bool {
        let hit = |s: &str| {
            let s: Vec<char> = s.chars().map(fold).collect();
            self.alts.iter().any(|a| glob(a, &s))
        };
        let found = if !self.path {
            hit(name)
        } else {
            // a glob term matches the whole path or a tail after a `/`; a plain term is
            // `*…*` so the whole path is enough
            hit(path) || path.match_indices('/').any(|(i, _)| hit(&path[i + 1..]))
        };
        found != self.not
    }
}

/// Whole-string wildcard match, backtracking to the last `*`
fn glob(p: &[Glob], s: &[char]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        match p.get(pi) {
            Some(Glob::Star) => {
                star = Some((pi, si));
                pi += 1;
            }
            Some(Glob::Any) => {
                pi += 1;
                si += 1;
            }
            Some(Glob::Char(c)) if *c == s[si] => {
                pi += 1;
                si += 1;
            }
            _ => match star {
                Some((sp, ss)) => {
                    pi = sp + 1;
                    si = ss + 1;
                    star = Some((sp, ss + 1));
                }
                None => return false,
            },
        }
    }
    p[pi..].iter().all(|g| *g == Glob::Star)
}

impl Pattern {
    /// Parse leniently, like `parseFilter`: stray operators are ignored. Saved text is
    /// checked first with [`check`].
    pub fn parse(text: &str) -> Self {
        let text = text.trim();
        let mut groups: Vec<Vec<Term>> = Vec::new();
        let mut joining = false; // the last token was OR
        let mut not = false;
        for t in tokens(text) {
            if t.is_or() {
                joining = !groups.is_empty();
                continue;
            }
            if t.word("AND") {
                joining = false;
                continue;
            }
            if t.word("NOT") {
                not = true;
                continue;
            }
            let mut p = t.text.as_str();
            let mut neg = not || t.neg;
            not = false;
            if !t.quoted && p.starts_with('-') {
                neg = true;
                p = &p[1..];
            }
            if p.is_empty() {
                continue;
            }
            let term = Term::new(p, neg);
            match groups.last_mut() {
                Some(g) if joining => g.push(term),
                _ => groups.push(vec![term]),
            }
            joining = false;
        }
        Self {
            text: text.to_string(),
            groups,
        }
    }

    /// The trimmed source text
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// No terms: matches everything
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Whether a file matches. `path` is `Folder/Sub/name` from the Put.io root; lists that
    /// only have a name pass it twice.
    pub fn matches(&self, name: &str, path: &str) -> bool {
        self.groups
            .iter()
            .all(|g| g.iter().any(|t| t.matches(name, path)))
    }
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl Eq for Pattern {}

impl Serialize for Pattern {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.text)
    }
}

/// Strict check for saved text: `Err` for text that can't mean what was typed, `Ok` with
/// warnings for text that works but probably not as intended. Empty text is fine.
pub fn check(text: &str) -> Result<Vec<String>, String> {
    let text = text.trim();
    if text.chars().count() > MAX_LEN {
        return Err(format!("longer than {MAX_LEN} characters"));
    }
    let toks = tokens(text);
    if toks.iter().any(|t| t.unclosed) {
        return Err("unclosed quote".into());
    }
    for t in toks.iter().filter(|t| !t.is_or()) {
        let mut open = false;
        // inside quotes a lone `{` or `}` is a literal character
        for c in t.text.chars().filter(|_| !t.quoted) {
            match c {
                '{' if open => return Err("nested { } are not supported".into()),
                '{' => open = true,
                '}' if !open => return Err("unbalanced { }".into()),
                '}' => open = false,
                _ => {}
            }
        }
        if open {
            return Err("unbalanced { }".into());
        }
        if alternatives(&t.text) > MAX_ALTERNATIVES {
            return Err(format!("more than {MAX_ALTERNATIVES} alternatives"));
        }
    }
    if !text.is_empty() && Pattern::parse(text).is_empty() {
        return Err("only operators, no pattern".into());
    }

    let mut warnings = Vec::new();
    let mut warn = |w: &str| {
        if !warnings.iter().any(|x| x == w) {
            warnings.push(w.to_string());
        }
    };
    let is_or = |i: usize| toks.get(i).is_some_and(Token::is_or);
    for (i, t) in toks.iter().enumerate() {
        if t.is_or() {
            if i == 0 {
                warn("an OR at the start is ignored");
            } else if i + 1 == toks.len() {
                warn("an OR at the end is ignored");
            } else if (t.text.starts_with(['|', ',']) && t.text.len() > 1) || is_or(i + 1) {
                warn("a repeated OR is ignored");
            }
        }
        let dangling = if t.word("NOT") {
            toks.get(i + 1)
                .is_none_or(|n| n.is_or() || n.word("AND") || n.word("NOT"))
        } else {
            !t.quoted && t.text == "-"
        };
        if dangling {
            warn("a NOT or - with nothing after it is ignored");
        }
        if !t.quoted && looks_like_class(&t.text) {
            warn("brackets are literal: [Ss] or [0-9] match those characters, not one of them (case never matters; use ? for one character)");
        }
    }
    Ok(warnings)
}

/// `[Ss]` (one letter in both cases) or `[0-9]` (a range)
fn looks_like_class(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    chars.windows(4).any(|w| {
        w[0] == '['
            && w[3] == ']'
            && w[1] != w[2]
            && w[1].is_alphabetic()
            && fold(w[1]) == fold(w[2])
    }) || chars.windows(5).any(|w| {
        w[0] == '['
            && w[2] == '-'
            && w[4] == ']'
            && w[1].is_alphanumeric()
            && w[3].is_alphanumeric()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown<'a>(filter: &str, names: &[&'a str]) -> Vec<&'a str> {
        let p = Pattern::parse(filter);
        names.iter().copied().filter(|n| p.matches(n, n)).collect()
    }

    #[test]
    fn comma_lists_and_braces_keep_working() {
        let names = ["a.mkv", "b.MP4", "c.avi"];
        assert_eq!(shown("*.mkv,*.mp4", &names), ["a.mkv", "b.MP4"]);
        assert_eq!(shown("*.{mkv,mp4}", &names), ["a.mkv", "b.MP4"]);
    }

    #[test]
    fn path_terms_match_tails() {
        let p = Pattern::parse("Sample/*");
        assert!(p.matches("a.mkv", "Movies/Sample/a.mkv"));
        assert!(p.matches("a.mkv", "Sample/a.mkv"));
        assert!(!p.matches("a.mkv", "Movies/a.mkv"));
    }

    #[test]
    fn empty_matches_everything_and_compares_by_text() {
        assert!(Pattern::parse("  ").is_empty());
        assert!(Pattern::parse("").matches("x", "x"));
        assert_eq!(Pattern::parse(" *.mkv "), Pattern::parse("*.mkv"));
        assert_eq!(Pattern::parse(" *.mkv ").as_str(), "*.mkv");
        assert_eq!(
            serde_json::to_string(&Pattern::parse("*.mkv")).unwrap(),
            "\"*.mkv\""
        );
    }

    #[test]
    fn expansion_is_capped() {
        let text = "{a,b}{a,b}{a,b}{a,b}{a,b}{a,b}{a,b}";
        assert_eq!(alternatives(text), 128);
        assert_eq!(
            Pattern::parse(text).groups[0][0].alts.len(),
            MAX_ALTERNATIVES
        );
    }

    #[test]
    fn stars_backtrack() {
        assert!(shown("*a*b*c", &["xaybzc"]).len() == 1);
        assert!(shown("*a*b*c", &["xaybzcd"]).is_empty());
        assert!(shown("a**", &["a"]).len() == 1);
    }

    #[test]
    fn check_errors() {
        for (text, err) in [
            ("\"abc", "unclosed quote"),
            ("*.{mkv", "unbalanced { }"),
            ("*.mkv}", "unbalanced { }"),
            ("{a b}", "unbalanced { }"),
            ("{a{b,c}}", "nested { } are not supported"),
            (
                "{a,b}{a,b}{a,b}{a,b}{a,b}{a,b}{a,b}",
                "more than 64 alternatives",
            ),
            ("OR | NOT", "only operators, no pattern"),
            ("-", "only operators, no pattern"),
        ] {
            assert_eq!(check(text), Err(err.to_string()), "{text}");
        }
        assert_eq!(
            check(&"a".repeat(1001)),
            Err("longer than 1000 characters".into())
        );
        assert!(check(&"a".repeat(1000)).is_ok());
    }

    #[test]
    fn check_accepts_good_text() {
        for text in [
            "",
            "*.mkv",
            "*.mkv,*.mp4",
            "*.{mkv,mp4}",
            "*.mkv AND NOT *sample*",
            "Sample/* OR *.nfo",
            "\"a {b\"",
            "[YTS]",
            "\"*[Ss]*\"",
        ] {
            assert_eq!(check(text), Ok(vec![]), "{text}");
        }
    }

    #[test]
    fn check_warnings() {
        let one = |text: &str| check(text).unwrap();
        assert_eq!(one("OR *.mkv"), ["an OR at the start is ignored"]);
        assert_eq!(one("*.mkv,"), ["an OR at the end is ignored"]);
        assert_eq!(one("a OR OR b"), ["a repeated OR is ignored"]);
        assert_eq!(one("a,,b"), ["a repeated OR is ignored"]);
        assert_eq!(
            one("a NOT"),
            ["a NOT or - with nothing after it is ignored"]
        );
        assert_eq!(
            one("a - b"),
            ["a NOT or - with nothing after it is ignored"]
        );
        assert!(one("*[Ss]ample*")[0].starts_with("brackets are literal"));
        assert!(one("*S[0-9]*")[0].starts_with("brackets are literal"));
        assert!(one("[SS]").is_empty());
    }
}
