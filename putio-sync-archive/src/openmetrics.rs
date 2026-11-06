//! OpenMetrics text for `promtool tsdb create-blocks-from openmetrics` (R8)

use crate::prom::Series;

/// Render series as OpenMetrics text: one `# TYPE <name> gauge` per family, samples
/// sorted by name, then labels, then time, timestamps in seconds, ending `# EOF`.
pub fn render(series: &[Series]) -> String {
    let mut sorted: Vec<&Series> = series.iter().collect();
    sorted.sort_by(|a, b| (&a.name, &a.labels).cmp(&(&b.name, &b.labels)));

    let mut out = String::new();
    let mut family: Option<&str> = None;
    for s in sorted {
        if family != Some(s.name.as_str()) {
            out.push_str(&format!("# TYPE {} gauge\n", s.name));
            family = Some(&s.name);
        }
        let labels = if s.labels.is_empty() {
            String::new()
        } else {
            let pairs: Vec<String> = s
                .labels
                .iter()
                .map(|(k, v)| format!("{k}=\"{}\"", escape(v)))
                .collect();
            format!("{{{}}}", pairs.join(","))
        };
        let mut samples = s.samples.clone();
        samples.sort_by_key(|sample| sample.ts_ms);
        for sample in samples {
            out.push_str(&format!(
                "{}{} {} {}\n",
                s.name,
                labels,
                number(sample.value),
                seconds(sample.ts_ms)
            ));
        }
    }
    out.push_str("# EOF\n");
    out
}

/// A label value with backslash, quote and newline escaped
fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// A sample value as OpenMetrics writes it
fn number(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 { "+Inf" } else { "-Inf" }.to_string()
    } else {
        value.to_string()
    }
}

/// Milliseconds as seconds, with a fraction only when there is one
fn seconds(ts_ms: i64) -> String {
    let (secs, millis) = (ts_ms.div_euclid(1000), ts_ms.rem_euclid(1000));
    if millis == 0 {
        secs.to_string()
    } else {
        format!("{secs}.{millis:03}")
            .trim_end_matches('0')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prom::Sample;
    use std::collections::BTreeMap;

    fn series(name: &str, labels: &[(&str, &str)], samples: &[(i64, f64)]) -> Series {
        Series {
            name: name.to_string(),
            labels: labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>(),
            samples: samples
                .iter()
                .map(|&(ts_ms, value)| Sample { ts_ms, value })
                .collect(),
        }
    }

    /// Check it with promtool by hand:
    /// `promtool tsdb create-blocks-from openmetrics golden.om /tmp/blocks`. Put the
    /// expected text below in `golden.om` first; the command must print the block ids
    /// without an error.
    #[test]
    fn golden_output() {
        let input = vec![
            series(
                "putio_sync:up:min5m",
                &[("job", "putio-sync"), ("instance", "nas:9742")],
                &[(1_793_491_500_000, 1.0), (1_793_491_200_000, 0.0)],
            ),
            series(
                "putio_sync:build_info:max5m",
                &[("version", "0.1.1"), ("commit", "abc\"d\\e\nf")],
                &[(1_793_491_200_500, 1.0)],
            ),
            series(
                "putio_sync:build_info:max5m",
                &[("version", "0.1.0")],
                &[(1_793_491_200_000, 1.0)],
            ),
            series(
                "putio_sync:process_cpu_usage:avg5m",
                &[("instance", "nas:9742")],
                &[(1_793_491_200_000, 0.125), (1_793_491_500_000, f64::NAN)],
            ),
        ];
        let expected = "\
# TYPE putio_sync:build_info:max5m gauge
putio_sync:build_info:max5m{commit=\"abc\\\"d\\\\e\\nf\",version=\"0.1.1\"} 1 1793491200.5
putio_sync:build_info:max5m{version=\"0.1.0\"} 1 1793491200
# TYPE putio_sync:process_cpu_usage:avg5m gauge
putio_sync:process_cpu_usage:avg5m{instance=\"nas:9742\"} 0.125 1793491200
putio_sync:process_cpu_usage:avg5m{instance=\"nas:9742\"} NaN 1793491500
# TYPE putio_sync:up:min5m gauge
putio_sync:up:min5m{instance=\"nas:9742\",job=\"putio-sync\"} 0 1793491200
putio_sync:up:min5m{instance=\"nas:9742\",job=\"putio-sync\"} 1 1793491500
# EOF
";
        assert_eq!(render(&input), expected);
    }

    #[test]
    fn output_does_not_depend_on_input_order() {
        let a = series(
            "putio_sync:a:5m",
            &[("x", "1")],
            &[(1000, 1.0), (2000, 2.0)],
        );
        let b = series("putio_sync:b:5m", &[], &[(1000, 3.0)]);
        assert_eq!(render(&[a.clone(), b.clone()]), render(&[b, a]));
    }

    #[test]
    fn nothing_is_just_eof() {
        assert_eq!(render(&[]), "# EOF\n");
    }
}
