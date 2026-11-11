# Contract: Arrived steps (the seam for 023/024)

```rust
// putio-sync-download/src/arrived.rs
pub struct ArrivedFile { pub file_id: i64, pub name: String, pub folder: String, pub size: i64 }

pub enum StepResult { Done, Retry(String) }


pub trait ArrivedStep: Send + Sync {
    /// Stable name, stored in `stream_files.pending`; no spaces
    fn name(&self) -> &'static str;
    fn run<'a>(&'a self, file: &'a ArrivedFile) -> BoxFuture<'a, StepResult>;
}
```

- The daemon builds the list (`DownloadManager::set_arrived_steps`). 022 registers `mp4`
  (`Mp4Step`) when `download.stream_mp4` is on.
- A file gets every registered step name at first sight. Steps run in list order; a `Retry`
  stops that file's later steps for this pass so an organiser never runs before an earlier
  step it depends on.
- At most 20 step runs per pass, oldest files first.
- 023 adds e.g. `tv` (move into `Show/Season NN/`), 024 `movie`. A step that moves a file on
  Put.io keeps its id; the next pass sees it at its new path, still decided `Stream` as long
  as its new folder is in stream mode.
