/// How a check runs: its parallel jobs, request budget, and confidence threshold.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub jobs: usize,
    pub max_requests: usize,
    pub min_confidence: f64,
}
