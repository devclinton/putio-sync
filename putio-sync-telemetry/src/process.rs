//! Process figures (feature 041): CPU time, memory, threads, open files and disk I/O,
//! read from `/proc/self` on each scrape.
//!
//! See `specs/041-resource-telemetry-archive/contracts/metrics.md`. Off Linux there is no
//! `/proc`, so every family is absent and one warning is logged on the first scrape.
//! A figure that can't be read is left out of that scrape only.

use std::sync::{Mutex, Once};

use prometheus::core::{Collector, Desc};
use prometheus::proto::MetricFamily;
use prometheus::{Counter, IntCounter, IntGauge};

use crate::opts;

/// The soft open-file limit from `/proc/self/limits`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdLimit {
    Value(u64),
    Unlimited,
}

/// User + system CPU time in clock ticks, from `/proc/self/stat` fields 14 and 15.
/// Fields are counted after the last `)`, since the process name may hold spaces or `)`.
pub fn parse_stat_cpu_ticks(stat: &str) -> Option<u64> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace().skip(11);
    let utime: u64 = fields.next()?.parse().ok()?;
    let stime: u64 = fields.next()?.parse().ok()?;
    Some(utime + stime)
}

/// A numeric field of `/proc/self/status`, such as `Threads`. Values given in kB, such
/// as `VmRSS`, are returned in bytes.
pub fn parse_status_field(status: &str, key: &str) -> Option<u64> {
    let line = status.lines().find(|l| l.split(':').next() == Some(key))?;
    let mut parts = line.split(':').nth(1)?.split_whitespace();
    let value: u64 = parts.next()?.parse().ok()?;
    match parts.next() {
        None => Some(value),
        Some("kB") => value.checked_mul(1024),
        Some(_) => None,
    }
}

/// The soft "Max open files" value of `/proc/self/limits`.
pub fn parse_limits_nofile(limits: &str) -> Option<FdLimit> {
    let line = limits.lines().find(|l| l.starts_with("Max open files"))?;
    let soft = line["Max open files".len()..].split_whitespace().next()?;
    if soft == "unlimited" {
        return Some(FdLimit::Unlimited);
    }
    soft.parse().ok().map(FdLimit::Value)
}

/// A `key: value` field of `/proc/self/io`, such as `read_bytes`.
pub fn parse_io_field(io: &str, key: &str) -> Option<u64> {
    io.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        if k.trim() == key {
            v.trim().parse().ok()
        } else {
            None
        }
    })
}

/// One read of every figure; `None` where it couldn't be read.
#[derive(Debug, Default)]
struct Readings {
    cpu_seconds: Option<f64>,
    resident_bytes: Option<u64>,
    virtual_bytes: Option<u64>,
    threads: Option<u64>,
    open_fds: Option<u64>,
    max_fds: Option<FdLimit>,
    read_bytes: Option<u64>,
    written_bytes: Option<u64>,
}

#[cfg(target_os = "linux")]
fn read_proc() -> Readings {
    let file = |name: &str| std::fs::read_to_string(format!("/proc/self/{}", name)).ok();
    let stat = file("stat");
    let status = file("status");
    let limits = file("limits");
    let io = file("io");
    let ticks = rustix::param::clock_ticks_per_second();
    let status_field = |key| status.as_deref().and_then(|s| parse_status_field(s, key));
    let io_field = |key| io.as_deref().and_then(|s| parse_io_field(s, key));
    Readings {
        cpu_seconds: stat
            .as_deref()
            .and_then(parse_stat_cpu_ticks)
            .filter(|_| ticks > 0)
            .map(|t| t as f64 / ticks as f64),
        resident_bytes: status_field("VmRSS"),
        virtual_bytes: status_field("VmSize"),
        threads: status_field("Threads"),
        open_fds: std::fs::read_dir("/proc/self/fd")
            .ok()
            .map(|d| d.count() as u64),
        max_fds: limits.as_deref().and_then(parse_limits_nofile),
        read_bytes: io_field("read_bytes"),
        written_bytes: io_field("write_bytes"),
    }
}

#[cfg(not(target_os = "linux"))]
fn read_proc() -> Readings {
    Readings::default()
}

/// Reads the process figures at scrape time. Registered with every other family.
pub(crate) struct ProcessCollector {
    descs: Vec<Desc>,
    cpu: Counter,
    resident: IntGauge,
    virtual_mem: IntGauge,
    threads: IntGauge,
    open_fds: IntGauge,
    max_fds: IntGauge,
    disk_read: IntCounter,
    disk_written: IntCounter,
    // Counters only go up, so concurrent scrapes take turns working out each delta.
    lock: Mutex<()>,
    warned: Once,
}

impl ProcessCollector {
    pub(crate) fn new() -> Self {
        let gauge = |name, help| IntGauge::with_opts(opts(name, help)).unwrap();
        let counter = |name, help| IntCounter::with_opts(opts(name, help)).unwrap();
        let c = ProcessCollector {
            descs: Vec::new(),
            cpu: Counter::with_opts(opts(
                "process_cpu_seconds_total",
                "User and system CPU time used by putio-sync.",
            ))
            .unwrap(),
            resident: gauge(
                "process_resident_memory_bytes",
                "Memory in use by putio-sync (resident set size).",
            ),
            virtual_mem: gauge(
                "process_virtual_memory_bytes",
                "Memory reserved by putio-sync (virtual size).",
            ),
            threads: gauge("process_threads", "OS threads in putio-sync."),
            open_fds: gauge("process_open_fds", "Open file descriptors."),
            max_fds: gauge(
                "process_max_fds",
                "Soft open-file limit; absent when unlimited.",
            ),
            disk_read: counter(
                "process_disk_read_bytes_total",
                "Bytes putio-sync read from storage.",
            ),
            disk_written: counter(
                "process_disk_written_bytes_total",
                "Bytes putio-sync wrote to storage.",
            ),
            lock: Mutex::new(()),
            warned: Once::new(),
        };
        let descs = c
            .metrics()
            .iter()
            .flat_map(|m| m.desc().into_iter().cloned())
            .collect();
        ProcessCollector { descs, ..c }
    }

    fn metrics(&self) -> [&dyn Collector; 8] {
        [
            &self.cpu,
            &self.resident,
            &self.virtual_mem,
            &self.threads,
            &self.open_fds,
            &self.max_fds,
            &self.disk_read,
            &self.disk_written,
        ]
    }
}

fn set_gauge(g: &IntGauge, v: u64) {
    g.set(i64::try_from(v).unwrap_or(i64::MAX));
}

fn raise_to(c: &IntCounter, v: u64) {
    if v > c.get() {
        c.inc_by(v - c.get());
    }
}

impl Collector for ProcessCollector {
    fn desc(&self) -> Vec<&Desc> {
        self.descs.iter().collect()
    }

    fn collect(&self) -> Vec<MetricFamily> {
        let _turn = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let r = read_proc();
        let mut out = Vec::new();
        let mut missing = Vec::new();
        let mut emit = |name: &'static str, present: bool, metric: &dyn Collector| {
            if present {
                out.extend(metric.collect());
            } else {
                missing.push(name);
            }
        };

        if let Some(secs) = r.cpu_seconds {
            let now = self.cpu.get();
            if secs > now {
                self.cpu.inc_by(secs - now);
            }
        }
        emit("cpu", r.cpu_seconds.is_some(), &self.cpu);
        for (name, value, gauge) in [
            ("resident_memory", r.resident_bytes, &self.resident),
            ("virtual_memory", r.virtual_bytes, &self.virtual_mem),
            ("threads", r.threads, &self.threads),
            ("open_fds", r.open_fds, &self.open_fds),
        ] {
            if let Some(v) = value {
                set_gauge(gauge, v);
            }
            emit(name, value.is_some(), gauge);
        }
        match r.max_fds {
            Some(FdLimit::Value(v)) => {
                set_gauge(&self.max_fds, v);
                emit("max_fds", true, &self.max_fds);
            }
            // Unlimited is a real answer, not a failed read: absent, but not reported.
            Some(FdLimit::Unlimited) => {}
            None => emit("max_fds", false, &self.max_fds),
        }
        for (name, value, counter) in [
            ("disk_read", r.read_bytes, &self.disk_read),
            ("disk_written", r.written_bytes, &self.disk_written),
        ] {
            if let Some(v) = value {
                raise_to(counter, v);
            }
            emit(name, value.is_some(), counter);
        }

        self.warned.call_once(|| {
            if !missing.is_empty() {
                tracing::warn!(
                    missing = %missing.join(","),
                    os = std::env::consts::OS,
                    "Process metrics unavailable"
                );
            }
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!(
            "{}/tests/fixtures/proc/{}",
            env!("CARGO_MANIFEST_DIR"),
            name
        );
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn stat_sums_user_and_system_ticks() {
        // The name holds a space and a `)`, so fields are counted after the last `)`.
        assert_eq!(parse_stat_cpu_ticks(&fixture("stat")), Some(1520 + 380));
        assert_eq!(parse_stat_cpu_ticks(&fixture("stat_malformed")), None);
        assert_eq!(parse_stat_cpu_ticks(""), None);
    }

    #[test]
    fn status_reads_memory_in_bytes_and_threads() {
        let s = fixture("status");
        assert_eq!(parse_status_field(&s, "VmRSS"), Some(36864 * 1024));
        assert_eq!(parse_status_field(&s, "VmSize"), Some(1228800 * 1024));
        assert_eq!(parse_status_field(&s, "Threads"), Some(9));
        assert_eq!(parse_status_field(&s, "VmSwap"), None);
        let bad = fixture("status_malformed");
        assert_eq!(parse_status_field(&bad, "VmRSS"), None);
    }

    #[test]
    fn limits_reads_the_soft_open_file_limit() {
        assert_eq!(
            parse_limits_nofile(&fixture("limits")),
            Some(FdLimit::Value(40960))
        );
        assert_eq!(
            parse_limits_nofile(&fixture("limits_unlimited")),
            Some(FdLimit::Unlimited)
        );
        assert_eq!(parse_limits_nofile(&fixture("limits_malformed")), None);
    }

    #[test]
    fn io_reads_storage_bytes() {
        let io = fixture("io");
        assert_eq!(parse_io_field(&io, "read_bytes"), Some(4_096_000));
        assert_eq!(parse_io_field(&io, "write_bytes"), Some(2_147_483_648));
        // `cancelled_write_bytes` must not match `write_bytes`.
        assert_eq!(parse_io_field(&io, "cancelled_write_bytes"), Some(0));
        assert_eq!(parse_io_field(&fixture("io_malformed"), "read_bytes"), None);
    }

    #[test]
    fn counters_only_rise() {
        let c = IntCounter::new("t", "t").unwrap();
        raise_to(&c, 10);
        raise_to(&c, 4);
        assert_eq!(c.get(), 10);
    }
}
