/// How a check runs: its parallel jobs, request budget, and confidence threshold.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Maximum contracts sharing one file request; one preserves isolated assessment.
    pub contract_batch_size: usize,
    pub jobs: usize,
    pub max_requests: usize,
    pub min_confidence: f64,
}
