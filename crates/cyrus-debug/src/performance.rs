//! Frame-rate plots and render statistics for the debug menu.
//!
//! [`FrameHistory`] records every real-time frame for the last [`HISTORY_SECONDS`]
//! seconds, whether or not the window is open, so the plots are already filled in
//! when it opens. [`RenderStats`] is only gathered while the window is open.

use std::collections::VecDeque;

use bevy::camera::visibility::ViewVisibility;
use bevy::ecs::entity::Entities;
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContexts, egui};

use crate::DebugMenu;

/// Width of the time axis in seconds.
const HISTORY_SECONDS: f64 = 30.0;
const PLOT_HEIGHT: f32 = 90.0;

pub(crate) struct PerformancePlugin;

impl Plugin for PerformancePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameHistory>()
            .init_resource::<RenderStats>()
            .add_systems(Last, record_frame)
            .add_systems(
                Update,
                collect_render_stats.run_if(|menu: Res<DebugMenu>| menu.performance_open),
            );
    }
}

struct Sample {
    /// Seconds since startup on the real-time clock.
    at: f64,
    frame_ms: f32,
}

/// Real-time frame durations for the last [`HISTORY_SECONDS`] seconds, oldest first.
#[derive(Resource, Default)]
pub(crate) struct FrameHistory {
    samples: VecDeque<Sample>,
}

impl FrameHistory {
    fn record(&mut self, at: f64, frame_ms: f32) {
        self.samples.push_back(Sample { at, frame_ms });
        while self
            .samples
            .front()
            .is_some_and(|oldest| at - oldest.at > HISTORY_SECONDS)
        {
            self.samples.pop_front();
        }
    }

    fn summary(&self) -> Option<FrameSummary> {
        let latest = self.samples.back()?;
        let mut sorted: Vec<f32> = self.samples.iter().map(|sample| sample.frame_ms).collect();
        sorted.sort_by(f32::total_cmp);
        let total_ms: f32 = sorted.iter().sum();
        let worst_ms = *sorted.last()?;
        // The slowest 1% of frames, so a few hitches show up in the "1% low" frame rate.
        let slow_ms = sorted[(sorted.len() * 99).div_ceil(100) - 1];
        Some(FrameSummary {
            current_fps: fps(latest.frame_ms),
            average_fps: fps(total_ms / sorted.len() as f32),
            low_fps: fps(slow_ms),
            worst_fps: fps(worst_ms),
            average_ms: total_ms / sorted.len() as f32,
            slow_ms,
            worst_ms,
        })
    }
}

fn fps(frame_ms: f32) -> f32 {
    if frame_ms > 0.0 {
        1000.0 / frame_ms
    } else {
        0.0
    }
}

struct FrameSummary {
    current_fps: f32,
    average_fps: f32,
    /// Frame rate of the 99th-percentile frame time.
    low_fps: f32,
    worst_fps: f32,
    average_ms: f32,
    /// 99th-percentile frame time.
    slow_ms: f32,
    worst_ms: f32,
}

/// Scene and renderer counts, refreshed every frame while the window is open.
#[derive(Resource, Default)]
pub(crate) struct RenderStats {
    entities: u32,
    meshes: usize,
    visible_meshes: usize,
    lights: usize,
    mesh_assets: Option<usize>,
    image_assets: Option<usize>,
    resolution: Option<UVec2>,
    adapter: Option<String>,
}

fn record_frame(time: Res<Time<Real>>, mut history: ResMut<FrameHistory>) {
    let delta = time.delta_secs();
    if delta > 0.0 {
        history.record(time.elapsed_secs_f64(), delta * 1000.0);
    }
}

// Each query reads a different component, so the stats cannot share one.
#[allow(clippy::too_many_arguments)]
fn collect_render_stats(
    mut stats: ResMut<RenderStats>,
    entities: &Entities,
    meshes: Query<&ViewVisibility, With<Mesh3d>>,
    point_lights: Query<(), With<PointLight>>,
    spot_lights: Query<(), With<SpotLight>>,
    directional_lights: Query<(), With<DirectionalLight>>,
    mesh_assets: Option<Res<Assets<Mesh>>>,
    image_assets: Option<Res<Assets<Image>>>,
    window: Query<&Window, With<PrimaryWindow>>,
    adapter: Option<Res<RenderAdapterInfo>>,
) {
    stats.entities = entities.count_spawned();
    stats.meshes = meshes.iter().count();
    stats.visible_meshes = meshes.iter().filter(|visible| visible.get()).count();
    stats.lights = point_lights.iter().count()
        + spot_lights.iter().count()
        + directional_lights.iter().count();
    stats.mesh_assets = mesh_assets.map(|assets| assets.len());
    stats.image_assets = image_assets.map(|assets| assets.len());
    stats.resolution = window.single().ok().map(Window::physical_size);
    if stats.adapter.is_none() {
        stats.adapter = adapter.map(|adapter| format!("{} ({:?})", adapter.name, adapter.backend));
    }
}

pub(crate) fn performance_window(
    mut contexts: EguiContexts,
    mut menu: ResMut<DebugMenu>,
    history: Res<FrameHistory>,
    stats: Res<RenderStats>,
) -> Result {
    let mut open = menu.performance_open;
    egui::Window::new("Performance")
        .id(egui::Id::new("Performance"))
        .open(&mut open)
        .default_width(380.0)
        .show(contexts.ctx_mut()?, |ui| {
            let Some(summary) = history.summary() else {
                ui.label("Waiting for frames...");
                return;
            };
            ui.heading(format!(
                "{:.0} FPS  ({:.1} ms)",
                summary.current_fps,
                1000.0 / summary.current_fps.max(f32::EPSILON)
            ));
            let fps_color = ui.visuals().hyperlink_color;
            let ms_color = ui.visuals().warn_fg_color;
            let now = history.samples.back().map_or(0.0, |sample| sample.at);

            ui.label("Frame rate (FPS)");
            plot(
                ui,
                history
                    .samples
                    .iter()
                    .map(|sample| (now - sample.at, fps(sample.frame_ms))),
                fps_color,
            );
            ui.label("Frame time (ms)");
            plot(
                ui,
                history
                    .samples
                    .iter()
                    .map(|sample| (now - sample.at, sample.frame_ms)),
                ms_color,
            );
            ui.weak(format!(
                "Last {HISTORY_SECONDS:.0} seconds, newest on the right."
            ));

            egui::Grid::new("frame summary")
                .num_columns(3)
                .spacing([16.0, 2.0])
                .show(ui, |ui| {
                    for (label, fps, ms) in [
                        ("Average", summary.average_fps, summary.average_ms),
                        ("1% low", summary.low_fps, summary.slow_ms),
                        ("Worst", summary.worst_fps, summary.worst_ms),
                    ] {
                        ui.label(label);
                        ui.label(format!("{fps:.0} FPS"));
                        ui.label(format!("{ms:.1} ms"));
                        ui.end_row();
                    }
                });

            ui.separator();
            egui::Grid::new("render stats")
                .num_columns(2)
                .spacing([16.0, 2.0])
                .show(ui, |ui| {
                    let mut row = |label: &str, value: String| {
                        ui.label(label);
                        ui.label(value);
                        ui.end_row();
                    };
                    if let Some(adapter) = &stats.adapter {
                        row("GPU", adapter.clone());
                    }
                    if let Some(size) = stats.resolution {
                        row("Resolution", format!("{} x {}", size.x, size.y));
                    }
                    row("Entities", stats.entities.to_string());
                    row(
                        "Meshes (visible / total)",
                        format!("{} / {}", stats.visible_meshes, stats.meshes),
                    );
                    row("Lights", stats.lights.to_string());
                    if let Some(count) = stats.mesh_assets {
                        row("Mesh assets", count.to_string());
                    }
                    if let Some(count) = stats.image_assets {
                        row("Image assets", count.to_string());
                    }
                });
        });
    menu.performance_open = open;
    Ok(())
}

/// Draws `points` (seconds ago, value) as a line over [`HISTORY_SECONDS`], with the
/// value axis running from zero to the next round number above the largest value.
fn plot(ui: &mut egui::Ui, points: impl Iterator<Item = (f64, f32)>, color: egui::Color32) {
    let points: Vec<(f64, f32)> = points.collect();
    let y_max = nice_ceiling(points.iter().map(|&(_, value)| value).fold(0.0, f32::max));
    let (response, painter) = ui.allocate_painter(
        egui::vec2(ui.available_width(), PLOT_HEIGHT),
        egui::Sense::hover(),
    );
    let rect = response.rect;
    painter.rect(
        rect,
        2.0,
        ui.visuals().extreme_bg_color,
        ui.visuals().widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );

    let grid = egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
    let text_color = ui.visuals().weak_text_color();
    let y_of = |value: f32| rect.bottom() - rect.height() * (value / y_max);
    for value in [0.0, y_max / 2.0, y_max] {
        let y = y_of(value);
        painter.hline(rect.x_range(), y, grid);
        // The top label sits below its gridline; the others sit above theirs.
        let (anchor, offset) = if value == y_max {
            (egui::Align2::LEFT_TOP, 1.0)
        } else {
            (egui::Align2::LEFT_BOTTOM, -1.0)
        };
        painter.text(
            egui::pos2(rect.left() + 4.0, y + offset),
            anchor,
            format!("{value:.0}"),
            egui::FontId::monospace(10.0),
            text_color,
        );
    }

    let line: Vec<egui::Pos2> = points
        .iter()
        .map(|&(age, value)| {
            let x = rect.right() - rect.width() * (age / HISTORY_SECONDS) as f32;
            egui::pos2(x, y_of(value))
        })
        .collect();
    painter.add(egui::Shape::line(line, egui::Stroke::new(1.5, color)));
}

/// Smallest 1, 2, 5 or 10 times a power of ten that is at least `value`.
fn nice_ceiling(value: f32) -> f32 {
    if value <= 0.0 {
        return 1.0;
    }
    let magnitude = 10f32.powf(value.log10().floor());
    let scaled = value / magnitude;
    let step = [1.0, 2.0, 5.0]
        .into_iter()
        .find(|&step| scaled <= step)
        .unwrap_or(10.0);
    step * magnitude
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_drops_frames_older_than_the_window() {
        let mut history = FrameHistory::default();
        for frame in 0..=400 {
            history.record(f64::from(frame) * 0.1, 16.0);
        }
        let oldest = history.samples.front().unwrap().at;
        let newest = history.samples.back().unwrap().at;
        assert!(newest - oldest <= HISTORY_SECONDS);
        assert!(newest - oldest > HISTORY_SECONDS - 0.2);
    }

    #[test]
    fn summary_reports_average_and_worst_frame() {
        let mut history = FrameHistory::default();
        assert!(history.summary().is_none());
        for frame in 0..99 {
            history.record(f64::from(frame) * 0.01, 10.0);
        }
        history.record(1.0, 40.0);
        let summary = history.summary().unwrap();
        assert_eq!(summary.current_fps, 25.0);
        assert_eq!(summary.worst_fps, 25.0);
        assert_eq!(summary.worst_ms, 40.0);
        // 99 frames at 10 ms and one at 40 ms: the 99th-percentile frame is still 10 ms.
        assert_eq!(summary.slow_ms, 10.0);
        assert!((summary.average_ms - 10.3).abs() < 1e-4);
        assert!((summary.average_fps - 1000.0 / 10.3).abs() < 1e-2);
    }

    #[test]
    fn axis_maximum_rounds_up_to_a_round_number() {
        for (value, expected) in [
            (0.0, 1.0),
            (0.8, 1.0),
            (16.7, 20.0),
            (60.0, 100.0),
            (45.0, 50.0),
            (144.0, 200.0),
            (1.0, 1.0),
        ] {
            let actual = nice_ceiling(value);
            assert!((actual - expected).abs() < 1e-4, "{value}: {actual}");
        }
    }
}
