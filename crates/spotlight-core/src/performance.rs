use std::{collections::VecDeque, sync::Mutex, time::Duration};

use crate::{ProviderId, QueryGeneration};

const DEFAULT_CAPACITY: usize = 256;

#[derive(Clone, Debug)]
pub struct SearchTiming {
    pub generation: QueryGeneration,
    pub provider: ProviderId,
    pub elapsed: Duration,
    pub result_count: usize,
    pub succeeded: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LatencySummary {
    pub sample_count: usize,
    pub p50_micros: u64,
    pub p95_micros: u64,
    pub maximum_micros: u64,
}

pub struct PerformanceRecorder {
    capacity: usize,
    samples: Mutex<VecDeque<SearchTiming>>,
}

impl Default for PerformanceRecorder {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl PerformanceRecorder {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            samples: Mutex::new(VecDeque::with_capacity(capacity.max(1))),
        }
    }

    pub fn record(&self, timing: SearchTiming) {
        let mut samples = self
            .samples
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if samples.len() == self.capacity {
            samples.pop_front();
        }
        samples.push_back(timing);
    }

    pub fn recent(&self) -> Vec<SearchTiming> {
        self.samples
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    pub fn summary_for(&self, provider: &ProviderId) -> LatencySummary {
        let mut micros = self
            .samples
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|sample| sample.succeeded && &sample.provider == provider)
            .map(|sample| sample.elapsed.as_micros().min(u128::from(u64::MAX)) as u64)
            .collect::<Vec<_>>();
        if micros.is_empty() {
            return LatencySummary::default();
        }
        micros.sort_unstable();
        LatencySummary {
            sample_count: micros.len(),
            p50_micros: percentile(&micros, 50),
            p95_micros: percentile(&micros, 95),
            maximum_micros: *micros.last().expect("non-empty samples"),
        }
    }
}

fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    let index = (sorted.len() - 1) * percentile / 100;
    sorted[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_is_bounded_and_summarizes_successes() {
        let recorder = PerformanceRecorder::new(3);
        for micros in [10, 20, 30, 40] {
            recorder.record(SearchTiming {
                generation: QueryGeneration(1),
                provider: "apps".into(),
                elapsed: Duration::from_micros(micros),
                result_count: 1,
                succeeded: true,
            });
        }
        recorder.record(SearchTiming {
            generation: QueryGeneration(2),
            provider: "apps".into(),
            elapsed: Duration::from_secs(9),
            result_count: 0,
            succeeded: false,
        });
        assert_eq!(recorder.recent().len(), 3);
        let summary = recorder.summary_for(&"apps".into());
        assert_eq!(summary.sample_count, 2);
        assert_eq!(summary.maximum_micros, 40);
    }
}
