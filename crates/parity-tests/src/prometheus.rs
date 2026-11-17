//! Reads counters out of the Prometheus text format that `/actuator/prometheus` serves.

/// The value of the sample `name{labels…}`, or 0 if there is none: a counter that was never incremented is
/// not exported. `labels` must all be present on the sample; other labels are ignored.
#[must_use]
pub fn sample(exposition: &str, name: &str, labels: &[(&str, &str)]) -> f64 {
    exposition
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let (series, value) = line.rsplit_once(' ')?;
            let (series_name, series_labels) = match series.split_once('{') {
                Some((series_name, rest)) => (series_name, rest.strip_suffix('}')?),
                None => (series, ""),
            };
            let matches = series_name == name
                && labels.iter().all(|(key, value)| {
                    series_labels
                        .split(',')
                        .any(|label| label == format!("{key}=\"{value}\""))
                });
            matches.then(|| value.parse::<f64>().ok())?
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPOSITION: &str = "\
# HELP ledgerly_expenses_transitions_total Successful transitions
# TYPE ledgerly_expenses_transitions_total counter
ledgerly_expenses_transitions_total{action=\"submit\",application=\"ledgerly\"} 3.0
ledgerly_expenses_transitions_total{action=\"approve\",application=\"ledgerly\"} 1.0
ledgerly_receipts_quarantined_total 2.0
jvm_memory_used_bytes{area=\"heap\",id=\"G1 Eden Space\"} 1.048576E7
";

    #[test]
    fn a_sample_is_found_by_name_and_labels() {
        assert_eq!(
            sample(
                EXPOSITION,
                "ledgerly_expenses_transitions_total",
                &[("action", "submit")]
            ),
            3.0
        );
        assert_eq!(
            sample(
                EXPOSITION,
                "ledgerly_expenses_transitions_total",
                &[("action", "approve")]
            ),
            1.0
        );
        assert_eq!(sample(EXPOSITION, "ledgerly_receipts_quarantined_total", &[]), 2.0);
    }

    #[test]
    fn without_labels_all_series_of_the_name_are_summed() {
        assert_eq!(sample(EXPOSITION, "ledgerly_expenses_transitions_total", &[]), 4.0);
    }

    #[test]
    fn a_counter_that_is_not_exported_counts_as_zero() {
        assert_eq!(sample(EXPOSITION, "ledgerly_imports_rows_skipped_total", &[]), 0.0);
        assert_eq!(
            sample(
                EXPOSITION,
                "ledgerly_expenses_transitions_total",
                &[("action", "reject")]
            ),
            0.0
        );
    }

    #[test]
    fn scientific_notation_and_label_values_with_spaces_are_read() {
        assert_eq!(
            sample(EXPOSITION, "jvm_memory_used_bytes", &[("area", "heap")]),
            10_485_760.0
        );
    }
}
