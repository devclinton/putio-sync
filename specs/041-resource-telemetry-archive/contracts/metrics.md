# Contract: New Metric Families

These are served on the existing endpoint (`[metrics]` bind and path, default
`0.0.0.0:9742/metrics`) at both `basic` and `detailed`, in daemon and server mode. None have
labels.

| Name | Type | Unit | Meaning |
|---|---|---|---|
| `putio_sync_process_cpu_seconds_total` | counter | seconds | User + system CPU time since start |
| `putio_sync_process_resident_memory_bytes` | gauge | bytes | Memory in use (RSS) |
| `putio_sync_process_virtual_memory_bytes` | gauge | bytes | Memory reserved (virtual size) |
| `putio_sync_process_threads` | gauge | — | OS threads |
| `putio_sync_process_open_fds` | gauge | — | Open file descriptors |
| `putio_sync_process_max_fds` | gauge | — | Soft open-file limit; absent if unlimited |
| `putio_sync_process_disk_read_bytes_total` | counter | bytes | Bytes read from storage since start |
| `putio_sync_process_disk_written_bytes_total` | counter | bytes | Bytes written to storage since start |
| `putio_sync_sync_in_progress` | gauge | 0/1 | 1 while a Put.io sync run is in progress |

## Behaviour

- **Fresh reads**: the process families are read at scrape time; there is no cached value.
- **Missing figures**: on a platform or kernel where a figure can't be read, its family is
  absent from the output. putio-sync logs once at WARN:
  `Process metrics unavailable missing="cpu,open_fds,..." os=macos`. Nothing is logged at all
  when every figure works.
- **Sync gauge**: `sync_in_progress` goes to 1 when a run begins and to 0 when it finishes,
  success or failure. It is 0 at startup.

## Invariants tested

1. With fixture `/proc` text, each parser returns the expected value, and malformed text
   returns `None`.
2. On Linux, a scrape contains all 8 process families with values above 0 for CPU, memory and
   threads. Two scrapes around a busy loop show CPU going up.
3. On non-Linux, a scrape contains none of the process families, and exactly one WARN line is
   logged across several scrapes.
4. `sync_in_progress` is 1 during a mocked sync and 0 after both a success and a failure.
5. No new family has labels. The existing redaction test over the full output still passes.
6. A scrape with the collector registered still answers in under 1 s during a synthetic
   download (feature 003's check).
