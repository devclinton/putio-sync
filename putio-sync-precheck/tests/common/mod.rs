//! Synthetic .torrent files, bencoded by hand (Constitution III: no real torrents)

#![allow(dead_code)]

/// A bencode value to encode
pub enum B {
    Int(i64),
    Str(Vec<u8>),
    List(Vec<B>),
    /// Keys are sorted on encode, as bencode requires
    Dict(Vec<(&'static str, B)>),
}

pub fn s(value: &str) -> B {
    B::Str(value.as_bytes().to_vec())
}

pub fn encode(value: &B) -> Vec<u8> {
    let mut out = Vec::new();
    write(value, &mut out);
    out
}

fn write(value: &B, out: &mut Vec<u8>) {
    match value {
        B::Int(n) => out.extend_from_slice(format!("i{n}e").as_bytes()),
        B::Str(bytes) => {
            out.extend_from_slice(format!("{}:", bytes.len()).as_bytes());
            out.extend_from_slice(bytes);
        }
        B::List(items) => {
            out.push(b'l');
            for item in items {
                write(item, out);
            }
            out.push(b'e');
        }
        B::Dict(entries) => {
            let mut sorted: Vec<&(&str, B)> = entries.iter().collect();
            sorted.sort_by_key(|(k, _)| *k);
            out.push(b'd');
            for (key, value) in sorted {
                write(&s(key), out);
                write(value, out);
            }
            out.push(b'e');
        }
    }
}

pub const MB: i64 = 1024 * 1024;

/// One entry of a v1 `files` list
pub fn file(path: &[&str], length: i64) -> B {
    B::Dict(vec![
        ("length", B::Int(length)),
        ("path", B::List(path.iter().map(|p| s(p)).collect())),
    ])
}

/// A multi-file torrent called `name`
pub fn multi(name: &str, files: Vec<B>) -> Vec<u8> {
    torrent(B::Dict(vec![
        ("files", B::List(files)),
        ("name", s(name)),
        ("piece length", B::Int(16384)),
        ("pieces", s("aaaaaaaaaaaaaaaaaaaa")),
    ]))
}

/// A single-file torrent
pub fn single(name: &str, length: i64) -> Vec<u8> {
    torrent(B::Dict(vec![
        ("length", B::Int(length)),
        ("name", s(name)),
        ("piece length", B::Int(16384)),
        ("pieces", s("aaaaaaaaaaaaaaaaaaaa")),
    ]))
}

/// A torrent around `info`, with a tracker list
pub fn torrent(info: B) -> Vec<u8> {
    encode(&B::Dict(vec![
        ("announce", s("udp://tracker.example.org:1337/announce")),
        (
            "announce-list",
            B::List(vec![
                B::List(vec![s("udp://tracker.example.org:1337/announce")]),
                B::List(vec![s("https://Other.Example.net/ann?passkey=SECRET")]),
            ]),
        ),
        ("info", info),
    ]))
}
