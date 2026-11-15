//! Native framebuffer evidence. Enabled only by the explicit playtest video flag.
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};

#[derive(Default, Resource)]
pub(super) struct CaptureQueue(pub Vec<String>);

pub(super) fn record(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut next: Local<f64>,
    mut frame: Local<u32>,
    mut captures: ResMut<CaptureQueue>,
    driver: Res<super::Driver>,
) {
    if std::env::var_os("TK_PLAYTEST_VIDEO").is_none()
        || time.elapsed_secs_f64() < 0.5
        || (driver.step == super::Step::Done && time.elapsed_secs_f64() - driver.since > 0.5)
        || (time.elapsed_secs_f64() < *next && captures.0.is_empty())
    {
        return;
    }
    let root = std::path::PathBuf::from(std::env::var("TK_PLAYTEST_DIR").unwrap());
    let frames = root.join("frames");
    std::fs::create_dir_all(&frames).expect("Create video frames directory");
    let mut screenshot = commands.spawn(Screenshot::primary_window());
    screenshot.observe(save_to_disk(frames.join(format!("{:06}.png", *frame))));
    for name in captures.0.drain(..) {
        screenshot.observe(save_to_disk(root.join(format!("{name}.png"))));
    }
    use std::io::Write;
    let mut times = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("frame-times.csv"))
        .expect("Open frame timeline");
    writeln!(times, "{},{:.6}", *frame, time.elapsed_secs_f64()).unwrap();
    *frame += 1;
    // Do not catch up after GPU stalls: the timestamp sidecar preserves real wall time.
    *next = time.elapsed_secs_f64() + 1.0 / 30.0;
}
