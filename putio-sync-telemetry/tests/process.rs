//! Contract test for `specs/041-resource-telemetry-archive/contracts/metrics.md`,
//! invariants 2 and 5. Invariant 3 is in `process_warning.rs`.

use putio_sync_telemetry::*;

const PROCESS_FAMILIES: &[&str] = &[
    "putio_sync_process_cpu_seconds_total",
    "putio_sync_process_resident_memory_bytes",
    "putio_sync_process_virtual_memory_bytes",
    "putio_sync_process_threads",
    "putio_sync_process_open_fds",
    "putio_sync_process_max_fds",
    "putio_sync_process_disk_read_bytes_total",
    "putio_sync_process_disk_written_bytes_total",
];

/// `name value` lines of the families above, labels included.
fn process_lines(output: &str) -> Vec<&str> {
    output
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter(|l| PROCESS_FAMILIES.iter().any(|f| l.starts_with(f)))
        .collect()
}

#[cfg(target_os = "linux")]
#[test]
fn linux_scrape_has_every_figure_and_cpu_rises() {
    let t = Telemetry::new();
    t.init(1, "1.2.3", "abc1234");
    let first = t.encode();
    // Containers can hide /proc/self/io, and an unlimited soft limit drops max_fds.
    for family in &PROCESS_FAMILIES[..5] {
        assert!(
            first.contains(&format!("\n{} ", family)),
            "missing {}",
            family
        );
    }
    for family in [
        "putio_sync_process_resident_memory_bytes",
        "putio_sync_process_threads",
    ] {
        assert!(t.sample(family, &[]).unwrap() > 0.0, "{} is 0", family);
    }
    let cpu = || {
        t.sample("putio_sync_process_cpu_seconds_total", &[])
            .unwrap()
    };
    let before = cpu();
    let started = std::time::Instant::now();
    let mut x = 0u64;
    while started.elapsed() < std::time::Duration::from_millis(200) {
        x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(7));
    }
    assert!(cpu() > before, "CPU stayed at {}", before);
}

#[test]
fn process_figures_have_no_labels() {
    let t = Telemetry::new();
    t.set_detail(Detail::Detailed);
    t.init(2, "1.2.3", "abc1234");
    for line in process_lines(&t.encode()) {
        let name = line.split_whitespace().next().unwrap();
        assert!(!name.contains('{'), "labels on {}", line);
    }
}

#[test]
fn sync_in_progress_is_present_and_zero_at_start() {
    let t = Telemetry::new();
    t.init(1, "1.2.3", "abc1234");
    assert!(t.encode().contains("\nputio_sync_sync_in_progress 0\n"));
}
