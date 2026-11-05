//! Path rules for the `path_map` field type and `putio.path` (feature 031): a path on
//! this machine → the path another program (a media server) sees for it.

use serde_json::Value;

/// Most rules in one `path_map` value
pub const MAX_RULES: usize = 16;

/// Whether `path` is a Windows path: `C:\…`, `C:/…` or `\\host\share`
pub fn is_windows(path: &str) -> bool {
    let b = path.as_bytes();
    path.starts_with("\\\\")
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'\\' | b'/'))
}

/// The length of the root: `/`, `C:\` or the `\\host\share` part
fn root_len(path: &str) -> usize {
    if let Some(rest) = path.strip_prefix("\\\\") {
        let mut parts = rest.match_indices(['\\', '/']);
        return match (parts.next(), parts.next()) {
            (Some(_), Some((second, _))) => second + 2,
            _ => path.len(),
        };
    }
    if is_windows(path) {
        3
    } else if path.starts_with('/') {
        1
    } else {
        0
    }
}

/// `path` without trailing separators, keeping a bare root
pub fn trim(path: &str) -> &str {
    let root = root_len(path);
    let mut end = path.len();
    while end > root && path[..end].ends_with(['/', '\\']) {
        end -= 1;
    }
    &path[..end]
}

/// `path` with `\` as `/`, for comparing Windows paths
fn slashed(path: &str) -> String {
    path.replace('\\', "/")
}

/// Whether `path` is `folder` or below it, on a separator boundary. Without case when
/// either is a Windows path.
pub fn within(path: &str, folder: &str) -> bool {
    let windows = is_windows(path) || is_windows(folder);
    let (mut p, mut f) = (slashed(trim(path)), slashed(trim(folder)));
    if windows {
        p = p.to_lowercase();
        f = f.to_lowercase();
    }
    if f.is_empty() {
        return false;
    }
    p == f || (p.starts_with(&f) && (f.ends_with('/') || p[f.len()..].starts_with('/')))
}

/// The folder holding `path`: `/a/b/c.mkv` → `/a/b`; a root stays itself
pub fn parent(path: &str) -> String {
    let t = trim(path);
    let root = root_len(t);
    let seps: &[char] = if is_windows(t) { &['\\', '/'] } else { &['/'] };
    match t.rfind(seps) {
        Some(i) if i >= root => t[..i.max(root)].to_string(),
        Some(_) => t[..root].to_string(),
        None => t.to_string(),
    }
}

/// Map `path` with `rules`: the longest `from` that is `path` or a folder above it wins;
/// the rest of the path is joined to its `to`, with `\` when `to` is a Windows path.
/// `path` comes back unchanged when no rule matches.
pub fn map(path: &str, rules: &[(String, String)]) -> String {
    let p = trim(path);
    let best = rules
        .iter()
        .map(|(from, to)| (trim(from), trim(to)))
        .filter(|(from, _)| within(p, from))
        .max_by_key(|(from, _)| from.len());
    let Some((from, to)) = best else {
        return p.to_string();
    };
    let rest = p[from.len()..].trim_start_matches('/');
    if rest.is_empty() {
        return to.to_string();
    }
    let (sep, rest) = if is_windows(to) {
        ('\\', rest.replace('/', "\\"))
    } else {
        ('/', rest.to_string())
    };
    if to.ends_with(['/', '\\']) {
        format!("{}{}", to, rest)
    } else {
        format!("{}{}{}", to, sep, rest)
    }
}

/// The rules in a `path_map` value, or what is wrong with it (no path in the message
/// beyond the rule number)
pub fn rules(value: &Value) -> Result<Vec<(String, String)>, String> {
    let items = value
        .as_array()
        .ok_or_else(|| "must be a list of {from, to} rules".to_string())?;
    if items.len() > MAX_RULES {
        return Err(format!("at most {} rules", MAX_RULES));
    }
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let n = i + 1;
        let obj = item
            .as_object()
            .ok_or_else(|| format!("rule {}: must be {{from, to}}", n))?;
        if obj.keys().any(|k| k != "from" && k != "to") {
            return Err(format!("rule {}: only from and to", n));
        }
        let get = |k: &str| obj.get(k).and_then(Value::as_str).map(str::trim);
        let (Some(from), Some(to)) = (get("from"), get("to")) else {
            return Err(format!("rule {}: needs from and to as text", n));
        };
        if !from.starts_with('/') {
            return Err(format!("rule {}: from must start with /", n));
        }
        if !(to.starts_with('/') || is_windows(to)) {
            return Err(format!(
                "rule {}: to must start with /, a drive (C:\\) or \\\\host",
                n
            ));
        }
        out.push((from.to_string(), to.to_string()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn r(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn longest_rule_on_folder_boundary() {
        let rules = r(&[("/downloads", "/data"), ("/downloads/TV/", "/tv")]);
        assert_eq!(map("/downloads/TV/Show/S01", &rules), "/tv/Show/S01");
        assert_eq!(map("/downloads/Movies/x", &rules), "/data/Movies/x");
        assert_eq!(map("/downloads", &rules), "/data");
        assert_eq!(map("/downloads2/x", &rules), "/downloads2/x");
        assert_eq!(map("/other/", &rules), "/other");
        assert_eq!(map("/x/y", &r(&[("/", "/mnt")])), "/mnt/x/y");
    }

    #[test]
    fn windows_targets() {
        let rules = r(&[("/downloads", "D:\\Media")]);
        assert_eq!(map("/downloads/TV/x", &rules), "D:\\Media\\TV\\x");
        assert_eq!(map("/downloads/a", &r(&[("/downloads", "D:\\")])), "D:\\a");
        assert_eq!(
            map("/downloads/a", &r(&[("/downloads", "\\\\nas\\media\\")])),
            "\\\\nas\\media\\a"
        );
    }

    #[test]
    fn within_and_parent() {
        assert!(within("/data/tv/Show", "/data/tv/"));
        assert!(within("/data/tv", "/data/tv"));
        assert!(!within("/data/tv2", "/data/tv"));
        assert!(within("/anything", "/"));
        assert!(within("D:\\Media\\TV\\x", "d:\\media\\tv"));
        assert!(within("D:\\Media\\TV\\x", "D:/Media"));
        assert!(!within("/Data/tv", "/data"));
        assert_eq!(parent("/a/b/c.mkv"), "/a/b");
        assert_eq!(parent("/a"), "/");
        assert_eq!(parent("/"), "/");
        assert_eq!(parent("C:\\a\\b"), "C:\\a");
        assert_eq!(parent("C:\\a"), "C:\\");
        assert_eq!(parent("\\\\nas\\share\\x"), "\\\\nas\\share");
    }

    #[test]
    fn rule_checks() {
        assert_eq!(
            rules(&json!([{"from": "/d/", "to": "C:\\m"}])).unwrap(),
            r(&[("/d/", "C:\\m")])
        );
        assert!(rules(&json!([])).unwrap().is_empty());
        let e = |v: Value| rules(&v).unwrap_err();
        assert!(e(json!("x")).contains("list"));
        assert!(e(json!([{"from": "d", "to": "/m"}])).contains("rule 1: from"));
        assert!(e(json!([{"from": "/d", "to": "m"}])).contains("rule 1: to"));
        assert!(e(json!([{"from": "/d"}])).contains("needs from and to"));
        assert!(e(json!([{"from": "/d", "to": "/m", "x": 1}])).contains("only from and to"));
        let many: Vec<Value> = (0..17).map(|_| json!({"from": "/a", "to": "/b"})).collect();
        assert!(e(Value::Array(many)).contains("at most 16"));
    }
}
