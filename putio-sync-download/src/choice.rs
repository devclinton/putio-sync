//! Pick files inside a transfer (feature 070): which choice a Put.io file falls under
//! and whether that choice wants it. A file's path in a choice is its path under the
//! item the transfer made on Put.io, each name sanitised the way folder rule paths are,
//! so the picker, a `.torrent` preview and the sync all name a file the same way. Pure,
//! so it is tested without Put.io.

use putio_sync_db::Choice;
use std::collections::HashMap;

/// The reason recorded for a file a choice leaves on Put.io
pub const REASON: &str = "Left out by your choice";

/// One listed Put.io item: its parent and its sanitised name
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub parent: Option<i64>,
    pub name: String,
}

/// Listed Put.io items by id
pub type Nodes = HashMap<i64, Node>;

/// The path of `file_id` under the nearest ancestor (or itself) that `is_root` accepts,
/// with that root's id. A transfer that is one file has the file's name as its path.
pub fn locate(file_id: i64, nodes: &Nodes, is_root: impl Fn(i64) -> bool) -> Option<(i64, String)> {
    // Guards against a parent cycle in a malformed listing
    const MAX_DEPTH: usize = 64;
    let mut segments: Vec<&str> = Vec::new();
    let mut current = file_id;
    for _ in 0..MAX_DEPTH {
        if is_root(current) {
            if segments.is_empty() {
                segments.push(&nodes.get(&current)?.name);
            }
            segments.reverse();
            return Some((current, segments.join("/")));
        }
        let node = nodes.get(&current)?;
        segments.push(&node.name);
        current = node.parent?;
    }
    None
}

/// The choices a sync pass applies, looked up by the root item of each
#[derive(Debug, Clone, Default)]
pub struct Choices {
    by_root: HashMap<i64, Choice>,
}

impl Choices {
    /// Choices whose root item is known; the others can match no file yet
    pub fn new(choices: impl IntoIterator<Item = Choice>) -> Self {
        Self {
            by_root: choices
                .into_iter()
                .filter_map(|c| Some((c.root_file_id?, c)))
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.by_root.is_empty()
    }

    /// Whether the choice `file_id` falls under wants it; `None` when it falls under
    /// none, so the folder rules decide
    pub fn wanted(&self, file_id: i64, nodes: &Nodes) -> Option<bool> {
        if self.by_root.is_empty() {
            return None;
        }
        let (root, path) = locate(file_id, nodes, |id| self.by_root.contains_key(&id))?;
        Some(self.by_root[&root].wants(&path))
    }
}

/// The paths a `.torrent`'s files will have in a choice, from their paths under the
/// torrent name (`Name/Season 1/e01.mkv`): Put.io makes a folder named after a torrent
/// of several files, so the name is dropped; a torrent of one file keeps its name.
pub fn torrent_paths(files: &[(String, u64)]) -> Vec<(String, u64)> {
    let sanitise = |segments: std::str::Split<'_, char>| {
        segments
            .filter(|s| !s.is_empty())
            .map(putio_sync_core::rules::sanitize_name)
            .collect::<Vec<_>>()
            .join("/")
    };
    files
        .iter()
        .map(|(path, size)| {
            let mut segments = path.split('/');
            let path = if path.contains('/') {
                segments.next();
                sanitise(segments)
            } else {
                sanitise(segments)
            };
            (path, *size)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes(items: &[(i64, Option<i64>, &str)]) -> Nodes {
        items
            .iter()
            .map(|(id, parent, name)| {
                (
                    *id,
                    Node {
                        parent: *parent,
                        name: name.to_string(),
                    },
                )
            })
            .collect()
    }

    fn choice(root: Option<i64>, others_wanted: bool, files: &[(&str, bool)]) -> Choice {
        Choice {
            transfer_id: 1,
            root_file_id: root,
            others_wanted,
            pending: false,
            files: files.iter().map(|(p, w)| (p.to_string(), *w)).collect(),
        }
    }

    /// Show (10) / Season 1 (11) / e01.mkv (12), e02.mkv (13); Show/readme.txt (14);
    /// a lone file (20) and an unrelated one (30)
    fn tree() -> Nodes {
        nodes(&[
            (10, Some(0), "Show"),
            (11, Some(10), "Season 1"),
            (12, Some(11), "e01.mkv"),
            (13, Some(11), "e02.mkv"),
            (14, Some(10), "readme.txt"),
            (20, Some(0), "film.mkv"),
            (30, Some(0), "other.mkv"),
        ])
    }

    #[test]
    fn a_path_is_relative_to_the_root_item() {
        let tree = tree();
        assert_eq!(
            locate(12, &tree, |id| id == 10),
            Some((10, "Season 1/e01.mkv".to_string()))
        );
        assert_eq!(
            locate(14, &tree, |id| id == 10),
            Some((10, "readme.txt".to_string()))
        );
        // A transfer that is one file
        assert_eq!(
            locate(20, &tree, |id| id == 20),
            Some((20, "film.mkv".to_string()))
        );
        assert_eq!(locate(30, &tree, |id| id == 10), None);
    }

    #[test]
    fn a_parent_cycle_ends() {
        let looped = nodes(&[(1, Some(2), "a"), (2, Some(1), "b")]);
        assert_eq!(locate(1, &looped, |_| false), None);
    }

    #[test]
    fn only_this_file_leaves_the_rest_and_later_files() {
        let choices = Choices::new([choice(Some(10), false, &[("Season 1/e01.mkv", true)])]);
        let tree = tree();
        assert_eq!(choices.wanted(12, &tree), Some(true));
        assert_eq!(choices.wanted(13, &tree), Some(false));
        assert_eq!(choices.wanted(14, &tree), Some(false));
        // Outside the transfer: the rules decide
        assert_eq!(choices.wanted(30, &tree), None);
    }

    #[test]
    fn all_but_this_keeps_later_files() {
        let choices = Choices::new([choice(Some(10), true, &[("readme.txt", false)])]);
        let tree = tree();
        assert_eq!(choices.wanted(14, &tree), Some(false));
        assert_eq!(choices.wanted(12, &tree), Some(true));
    }

    #[test]
    fn a_choice_with_no_root_matches_nothing() {
        let choices = Choices::new([choice(None, false, &[])]);
        assert!(choices.is_empty());
        assert_eq!(choices.wanted(12, &tree()), None);
    }

    #[test]
    fn a_pending_choice_wants_nothing() {
        let choices = Choices::new([Choice {
            pending: true,
            ..choice(Some(20), false, &[])
        }]);
        assert_eq!(choices.wanted(20, &tree()), Some(false));
    }

    #[test]
    fn torrent_paths_drop_the_top_folder() {
        let files = [
            ("Show/Season 1/e01.mkv".to_string(), 10),
            ("Show/readme.txt".to_string(), 1),
        ];
        assert_eq!(
            torrent_paths(&files),
            vec![
                ("Season 1/e01.mkv".to_string(), 10),
                ("readme.txt".to_string(), 1)
            ]
        );
        assert_eq!(
            torrent_paths(&[("film.mkv".to_string(), 5)]),
            vec![("film.mkv".to_string(), 5)]
        );
    }
}
