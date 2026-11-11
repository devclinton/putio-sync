//! Keep the title and the episode (SxxEyy) or year of a video's name; drop what follows.
//! The Rust port of examples/plugins/clean-names, plus a log line per finished download.

mod putio;

use std::cell::RefCell;
use std::collections::HashSet;

use serde_json::{json, Value};

thread_local! {
    static RENAMED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

#[no_mangle]
pub extern "C" fn init(ptr: *mut u8, len: usize) -> u64 {
    putio::args(ptr, len);
    let config: Value = match putio::call("config", json!([])) {
        Ok(c) => c,
        Err(e) => {
            putio::fail(&e);
            return 0;
        }
    };
    let extensions = config["extensions"].as_array().cloned().unwrap_or_default();
    RENAMED.with(|r| {
        r.borrow_mut().extend(
            extensions
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_lowercase),
        )
    });
    putio::result::<()>(None)
}

#[no_mangle]
pub extern "C" fn rename(ptr: *mut u8, len: usize) -> u64 {
    let args = putio::args(ptr, len);
    let file = &args[0];
    let name = file["name"].as_str().unwrap_or_default();
    let extension = file["extension"].as_str().unwrap_or_default();
    if extension.is_empty() || !RENAMED.with(|r| r.borrow().contains(extension)) {
        return putio::result::<String>(None);
    }
    putio::result(clean(name).filter(|new| new != name))
}

#[no_mangle]
pub extern "C" fn on_event(ptr: *mut u8, len: usize) -> u64 {
    let args = putio::args(ptr, len);
    let event = &args[0];
    let count: u64 = putio::call("store.get", json!(["renamed"])).unwrap_or(0) + 1;
    if let Err(e) = putio::call::<Value>("store.set", json!(["renamed", count])) {
        putio::fail(&e);
        return 0;
    }
    putio::info("download finished", json!({"type": event["type"], "count": count}));
    putio::result::<()>(None)
}

fn trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || c == '-')
}

/// `Show.S01E02.1080p.mkv` → `Show S01E02.mkv`; `The.Movie.2019.1080p.mp4` → `The Movie (2019).mp4`
fn clean(name: &str) -> Option<String> {
    let (base, ext) = name.rsplit_once('.')?;
    let spaced: String = base.chars().map(|c| if c == '.' || c == '_' { ' ' } else { c }).collect();
    let words: Vec<&str> = spaced.split(' ').collect();
    for (i, word) in words.iter().enumerate() {
        if let Some(episode) = episode_of(word) {
            let show = trim(&words[..i].join(" ")).to_string();
            return (!show.is_empty()).then(|| format!("{} {}.{}", show, episode, ext));
        }
    }
    for (i, word) in words.iter().enumerate().skip(1) {
        let year = word.trim_matches(|c| matches!(c, '(' | ')' | '[' | ']'));
        let is_year = year.len() == 4
            && year.chars().all(|c| c.is_ascii_digit())
            && (year.starts_with("19") || year.starts_with("20"));
        if is_year {
            let title = trim(&words[..i].join(" ")).to_string();
            return (!title.is_empty()).then(|| format!("{} ({}).{}", title, year, ext));
        }
    }
    None
}

/// `s01e02` → `S01E02`, at the start of `word`
fn episode_of(word: &str) -> Option<String> {
    let upper = word.to_ascii_uppercase();
    let rest = upper.strip_prefix('S')?;
    let season: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let rest = rest[season.len()..].strip_prefix('E')?;
    let episode: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let ok = (1..=2).contains(&season.len()) && (2..=3).contains(&episode.len());
    ok.then(|| format!("S{}E{}", season, episode))
}
