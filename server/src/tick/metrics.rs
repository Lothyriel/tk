use bevy::prelude::*;
use std::time::Instant;

#[derive(Resource)]
pub(super) struct TickMetrics {
    start: Instant,
    samples: Vec<f64>,
}
impl Default for TickMetrics {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            samples: Vec::with_capacity(640),
        }
    }
}
pub(super) fn begin(mut metrics: ResMut<TickMetrics>) {
    metrics.start = Instant::now();
}
pub(super) fn end(
    mut metrics: ResMut<TickMetrics>,
    pieces: Query<(), With<super::physical::PhysicsPiece>>,
) {
    let elapsed = metrics.start.elapsed().as_secs_f64() * 1000.0;
    metrics.samples.push(elapsed);
    if metrics.samples.len() == 640 {
        metrics.samples.sort_by(f64::total_cmp);
        info!(
            "TK PERF tick_ms mean={:.3} p95={:.3} max={:.3} pieces={}",
            metrics.samples.iter().sum::<f64>() / 640.0,
            metrics.samples[608],
            metrics.samples[639],
            pieces.iter().count()
        );
        metrics.samples.clear();
    }
}
