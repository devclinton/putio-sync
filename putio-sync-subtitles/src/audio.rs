//! Audio for transcription (feature 025): find `ffmpeg` and `ffprobe`, plan the chunks, cut
//! one chunk of a video into a 16 kHz mono WAV, and make the synthetic tone the Test button
//! sends.
//!
//! ffmpeg runs with `kill_on_drop`, so cancelling the step (dropping the future) stops it.
//! Chunk files are removed when their guard drops: after use, on an error and on cancel.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::process::Command;

/// The message when no ffmpeg can be found
pub const NOT_FOUND: &str = "ffmpeg not found: set transcription.ffmpeg_path or install ffmpeg";

/// Why audio could not be made
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AudioError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Failed(String),
}

/// ffmpeg and ffprobe
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn is_file(p: &Path) -> bool {
    p.is_file()
}

/// `name` in the directories of `path_var`
fn search(name: &str, path_var: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let file = exe_name(name);
    std::env::split_paths(path_var?)
        .map(|d| d.join(&file))
        .find(|p| is_file(p))
}

/// Find ffmpeg: the configured path, else `PATH`
pub fn find_ffmpeg(configured: &str) -> Option<PathBuf> {
    find_ffmpeg_in(configured, std::env::var_os("PATH").as_deref())
}

fn find_ffmpeg_in(configured: &str, path_var: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let configured = configured.trim();
    if !configured.is_empty() {
        let p = PathBuf::from(configured);
        return is_file(&p).then_some(p);
    }
    search("ffmpeg", path_var)
}

/// Find both tools: ffprobe is looked up next to ffmpeg, then on `PATH`
pub fn discover(configured: &str) -> Result<Tools, AudioError> {
    discover_in(configured, std::env::var_os("PATH").as_deref())
}

fn discover_in(configured: &str, path_var: Option<&std::ffi::OsStr>) -> Result<Tools, AudioError> {
    let ffmpeg = find_ffmpeg_in(configured, path_var)
        .ok_or_else(|| AudioError::NotFound(NOT_FOUND.to_string()))?;
    let next_to = ffmpeg
        .parent()
        .map(|d| d.join(exe_name("ffprobe")))
        .filter(|p| is_file(p));
    let ffprobe = next_to
        .or_else(|| search("ffprobe", path_var))
        .ok_or_else(|| {
            AudioError::NotFound(
                "ffprobe not found: it is installed with ffmpeg; put it next to ffmpeg or on PATH"
                    .to_string(),
            )
        })?;
    Ok(Tools { ffmpeg, ffprobe })
}

/// The first line of `ffmpeg -version`, for the Test button
pub async fn version(ffmpeg: &Path) -> Option<String> {
    let out = Command::new(ffmpeg)
        .arg("-version")
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?.trim();
    // "ffmpeg version 6.1.1 Copyright ..." -> "6.1.1"
    let v = line
        .strip_prefix("ffmpeg version ")
        .and_then(|r| r.split_whitespace().next())
        .unwrap_or(line);
    (!v.is_empty()).then(|| v.to_string())
}

/// The length of the first audio-bearing container, in seconds
pub async fn duration_secs(ffprobe: &Path, video: &Path) -> Result<f64, AudioError> {
    let out = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(video)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| AudioError::Failed(format!("could not run ffprobe: {e}")))?;
    if !out.status.success() {
        return Err(AudioError::Failed(format!(
            "ffprobe failed: {}",
            tail(&out.stderr)
        )));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|d| d.is_finite() && *d > 0.0)
        .ok_or_else(|| AudioError::Failed("ffprobe did not report a duration".to_string()))
}

/// One part of the audio
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chunk {
    pub index: usize,
    pub start_secs: f64,
    pub len_secs: f64,
}

/// Chunk `i` covers `[i*chunk_secs, min((i+1)*chunk_secs, duration))`
pub fn plan_chunks(duration: f64, chunk_secs: u64) -> Vec<Chunk> {
    let size = chunk_secs.max(1) as f64;
    let mut out = Vec::new();
    let mut start = 0.0;
    while start < duration - 0.001 {
        let len = (duration - start).min(size);
        out.push(Chunk {
            index: out.len(),
            start_secs: start,
            len_secs: len,
        });
        start += size;
    }
    out
}

/// A temp file removed when dropped
#[derive(Debug)]
pub struct TempWav {
    path: PathBuf,
}

impl TempWav {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempWav {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh temp path under `dir`
pub fn temp_wav(dir: &Path) -> TempWav {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    TempWav {
        path: dir.join(format!("putio-sync-audio-{}-{n}.wav", std::process::id())),
    }
}

/// The ffmpeg arguments for one chunk
pub fn chunk_args(video: &Path, chunk: &Chunk, out: &Path) -> Vec<std::ffi::OsString> {
    let mut a: Vec<std::ffi::OsString> = Vec::new();
    for s in ["-nostdin", "-v", "error", "-y", "-ss"] {
        a.push(s.into());
    }
    a.push(format!("{:.3}", chunk.start_secs).into());
    a.push("-t".into());
    a.push(format!("{:.3}", chunk.len_secs).into());
    a.push("-i".into());
    a.push(video.into());
    for s in [
        "-map",
        "0:a:0",
        "-vn",
        "-ac",
        "1",
        "-ar",
        "16000",
        "-c:a",
        "pcm_s16le",
        "-f",
        "wav",
    ] {
        a.push(s.into());
    }
    a.push(out.into());
    a
}

/// Cut `chunk` of `video` into a WAV in `dir`. The file is removed when the guard drops, so
/// dropping this future (cancel) leaves nothing behind either.
pub async fn extract_chunk(
    ffmpeg: &Path,
    video: &Path,
    chunk: &Chunk,
    dir: &Path,
) -> Result<TempWav, AudioError> {
    let wav = temp_wav(dir);
    let out = Command::new(ffmpeg)
        .args(chunk_args(video, chunk, wav.path()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| AudioError::Failed(format!("could not run ffmpeg: {e}")))?;
    if !out.status.success() {
        return Err(AudioError::Failed(format!(
            "ffmpeg failed: {}",
            tail(&out.stderr)
        )));
    }
    match std::fs::metadata(wav.path()) {
        Ok(m) if m.len() > 44 => Ok(wav),
        _ => Err(AudioError::Failed(
            "ffmpeg made no audio (does the video have an audio track?)".to_string(),
        )),
    }
}

/// The last couple of lines of a tool's stderr, short enough for a step message
fn tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let from = lines.len().saturating_sub(2);
    let joined = lines[from..].join(" | ");
    if joined.is_empty() {
        return "no message".to_string();
    }
    joined.chars().take(300).collect()
}

/// A WAV (16 kHz mono PCM s16le) with a 440 Hz tone, made in process
pub fn test_tone_wav(seconds: u32) -> Vec<u8> {
    const RATE: u32 = 16_000;
    let samples = RATE * seconds;
    let data_len = samples * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..samples {
        let t = i as f64 / RATE as f64;
        let v = (t * 440.0 * std::f64::consts::TAU).sin() * 0.3 * i16::MAX as f64;
        out.extend_from_slice(&(v as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_cover_the_duration_without_overlap() {
        let c = plan_chunks(1500.0, 600);
        assert_eq!(c.len(), 3);
        assert_eq!((c[0].start_secs, c[0].len_secs), (0.0, 600.0));
        assert_eq!((c[1].start_secs, c[1].len_secs), (600.0, 600.0));
        assert_eq!((c[2].start_secs, c[2].len_secs), (1200.0, 300.0));
        assert_eq!(plan_chunks(600.0, 600).len(), 1);
        assert_eq!(plan_chunks(600.5, 600).len(), 2);
        assert!(plan_chunks(0.0, 600).is_empty());
    }

    #[test]
    fn tone_is_a_valid_wav() {
        let wav = test_tone_wav(3);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 16_000 * 3 * 2);
        assert!(wav[44..].iter().any(|b| *b != 0), "not silence");
    }

    #[test]
    fn args_are_argv_with_no_shell() {
        let chunk = Chunk {
            index: 1,
            start_secs: 600.0,
            len_secs: 300.5,
        };
        let a: Vec<String> = chunk_args(Path::new("/m/a b.mkv"), &chunk, Path::new("/t/o.wav"))
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert_eq!(a[0], "-nostdin");
        assert!(a.windows(2).any(|w| w == ["-ss", "600.000"]));
        assert!(a.windows(2).any(|w| w == ["-t", "300.500"]));
        assert!(a.windows(2).any(|w| w == ["-i", "/m/a b.mkv"]));
        assert!(a.windows(2).any(|w| w == ["-ar", "16000"]));
        assert_eq!(a.last().unwrap(), "/t/o.wav");
    }
}
