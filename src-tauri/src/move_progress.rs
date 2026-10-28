use serde::Serialize;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MovePhase {
    Scanning,
    Copying,
    Switching,
    Cleaning,
    Complete,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveProgress {
    pub phase: MovePhase,
    pub copied_bytes: u64,
    pub total_bytes: u64,
    pub copied_files: u64,
    pub total_files: u64,
    pub processed_bytes: u64,
    pub total_processing_bytes: u64,
    pub estimated_seconds_remaining: Option<u64>,
}

pub struct MoveReporter<'a> {
    progress: MoveProgress,
    report: &'a mut dyn FnMut(MoveProgress),
    started: Option<Instant>,
    last_report: Instant,
}

impl<'a> MoveReporter<'a> {
    pub fn new(report: &'a mut dyn FnMut(MoveProgress)) -> Self {
        let mut reporter = Self {
            progress: MoveProgress {
                phase: MovePhase::Scanning,
                copied_bytes: 0,
                total_bytes: 0,
                copied_files: 0,
                total_files: 0,
                processed_bytes: 0,
                total_processing_bytes: 0,
                estimated_seconds_remaining: None,
            },
            report,
            started: None,
            last_report: Instant::now(),
        };
        reporter.emit(true);
        reporter
    }

    pub fn found_file(&mut self, bytes: u64) {
        self.progress.total_bytes += bytes;
        self.progress.total_files += 1;
        self.emit(false);
    }

    pub fn start_copy(&mut self) {
        // One copy, two integrity reads, and two source checks before removal.
        self.progress.total_processing_bytes = self.progress.total_bytes.saturating_mul(5);
        self.started = Some(Instant::now());
        self.phase(MovePhase::Copying);
    }

    pub fn phase(&mut self, phase: MovePhase) {
        self.progress.phase = phase;
        self.emit(true);
    }

    pub fn copied(&mut self, bytes: u64) {
        self.progress.copied_bytes += bytes;
        self.processed(bytes);
    }

    pub fn copied_file(&mut self) {
        self.progress.copied_files += 1;
        self.emit(false);
    }

    pub fn processed(&mut self, bytes: u64) {
        self.progress.processed_bytes += bytes;
        self.emit(false);
    }

    fn emit(&mut self, force: bool) {
        let now = Instant::now();
        if !force && now.duration_since(self.last_report) < Duration::from_millis(150) {
            return;
        }
        self.progress.estimated_seconds_remaining = match self.progress.phase {
            MovePhase::Copying | MovePhase::Cleaning => self.started.and_then(|started| {
                estimate_remaining(
                    self.progress.processed_bytes,
                    self.progress.total_processing_bytes,
                    now.duration_since(started),
                )
            }),
            _ => None,
        };
        (self.report)(self.progress.clone());
        self.last_report = now;
    }
}

fn estimate_remaining(processed: u64, total: u64, elapsed: Duration) -> Option<u64> {
    if elapsed < Duration::from_secs(1) || processed == 0 || processed >= total {
        return None;
    }
    Some(((total - processed) as f64 * elapsed.as_secs_f64() / processed as f64).ceil() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_uses_measured_throughput_and_waits_for_a_sample() {
        assert_eq!(
            estimate_remaining(250, 1000, Duration::from_secs(2)),
            Some(6)
        );
        assert_eq!(
            estimate_remaining(500, 1000, Duration::from_secs(2)),
            Some(2)
        );
        assert_eq!(
            estimate_remaining(250, 1000, Duration::from_millis(900)),
            None
        );
        assert_eq!(estimate_remaining(0, 1000, Duration::from_secs(2)), None);
        assert_eq!(estimate_remaining(1000, 1000, Duration::from_secs(2)), None);
        assert_eq!(estimate_remaining(0, 0, Duration::from_secs(2)), None);
    }
}
