//! Standalone rendered review. Uses the production voxel and icon caches.
use super::{VoxelCache, VoxelModel, instantiate};
use crate::render::item_icons::{self, ItemIcon};
use bevy::{
    camera::visibility::RenderLayers,
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
    window::{PresentMode, PrimaryWindow},
};
use common::catalog::item_manifest::{S1_SEEDS, item_visuals};

const BEACON: &str = "beacon_authoring";

pub(crate) struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        let benchmark = std::env::var_os("TK_ITEM_REVIEW_BENCH").is_some();
        let screenshot = std::env::var_os("TK_ITEM_REVIEW_SCREENSHOT").map(Into::into);
        assert!(
            !benchmark || screenshot.is_none(),
            "benchmark cannot capture a screenshot"
        );
        let angle = match std::env::var("TK_ITEM_REVIEW_ANGLE").as_deref() {
            Ok("side") => 90.0,
            Ok("back") => 180.0,
            Ok("front") | Err(_) => 0.0,
            Ok(value) => panic!("unknown item review angle: {value}"),
        };
        let focus = std::env::var("TK_ITEM_REVIEW_FOCUS").ok();
        if let Some(id) = &focus {
            assert!(
                S1_SEEDS.contains(&id.as_str()) || id == BEACON,
                "unknown review item: {id}"
            );
        }
        app.init_resource::<VoxelCache>()
            .insert_resource(Settings {
                angle,
                benchmark,
                screenshot,
                focus,
            })
            .add_plugins(item_icons::Plugin)
            .add_systems(Startup, setup)
            .add_systems(Update, (instantiate, review))
            .add_systems(Last, benchmark_frames);
    }
}

#[derive(Resource)]
struct Settings {
    angle: f32,
    benchmark: bool,
    screenshot: Option<std::path::PathBuf>,
    focus: Option<String>,
}

#[derive(Component)]
struct Prop;

fn setup(
    mut commands: Commands,
    settings: Res<Settings>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if settings.benchmark
        && let Ok(mut window) = windows.single_mut()
    {
        window.present_mode = PresentMode::AutoNoVsync;
    }
    let beacon = settings.focus.as_deref() == Some(BEACON);
    let camera = if beacon {
        Transform::from_xyz(0.0, 0.16, -0.52).looking_at(Vec3::ZERO, Vec3::Y)
    } else if let Some(id) = &settings.focus {
        let half = Vec3::from_array(item_visuals()[id].collider_half_extents());
        // Frame the enclosing sphere, so the same distance works for all
        // review angles without changing the authored scale.
        let radius = half.length();
        let center = Vec3::Y * half.y;
        Transform::from_translation(center + Vec3::new(0.0, 1.6, -3.2) * radius)
            .looking_at(center, Vec3::Y)
    } else {
        Transform::from_xyz(0.0, 0.85, -1.65).looking_at(Vec3::new(0.0, 0.08, 0.0), Vec3::Y)
    };
    let mut world = commands.spawn((Camera3d::default(), camera));
    if beacon {
        world.insert(Msaa::Off);
        commands.spawn((
            Camera3d::default(),
            Msaa::Off,
            Camera {
                order: 1,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            RenderLayers::layer(super::VIEW_MODEL_RENDER_LAYER),
            camera,
        ));
        for (view_model, x) in [(false, 0.13), (true, -0.13)] {
            commands.spawn((
                VoxelModel {
                    asset: BEACON.into(),
                    view_model,
                },
                Transform::from_xyz(x, 0.0, 0.0)
                    .with_rotation(Quat::from_rotation_y(settings.angle.to_radians())),
                Visibility::Inherited,
                Prop,
            ));
        }
    }
    commands.spawn((
        DirectionalLight {
            illuminance: 9000.0,
            ..default()
        },
        Transform::from_xyz(1.0, 2.0, -1.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::from_layers(&[0, super::VIEW_MODEL_RENDER_LAYER]),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 500.0,
        ..default()
    });
    for (index, id) in S1_SEEDS.into_iter().enumerate() {
        if settings.focus.as_deref().is_some_and(|focus| focus != id) {
            continue;
        }
        let binding = &item_visuals()[id];
        let center = Vec3::from_array(binding.collider_center());
        let pad = if settings.focus.is_some() {
            Vec3::ZERO
        } else {
            Vec3::new((index as f32 - 2.5) * -0.21, 0.0, 0.0)
        };
        let offset = Vec3::new(center.x, binding.min[1], center.z);
        commands
            .spawn((
                Transform::from_translation(pad)
                    .with_rotation(Quat::from_rotation_y(settings.angle.to_radians())),
                Visibility::Inherited,
                Prop,
            ))
            .with_child((
                VoxelModel {
                    asset: binding.asset_key.clone(),
                    view_model: false,
                },
                Transform::from_translation(-offset),
                Visibility::Inherited,
            ));
    }
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: px(24),
            width: percent(100),
            justify_content: JustifyContent::Center,
            column_gap: px(18),
            ..default()
        })
        .with_children(|row| {
            for id in S1_SEEDS {
                if settings.focus.as_deref().is_some_and(|focus| focus != id) {
                    continue;
                }
                row.spawn((
                    ItemIcon(id.into()),
                    Node {
                        width: px(90),
                        height: px(90),
                        ..default()
                    },
                ));
            }
        });
    commands.spawn((
        Text::new(if beacon {
            "Beacon authoring review: world (left) / view-model (right)  |  1 Front  2 Side  3 Back  |  F12 Screenshot".into()
        } else {
            format!(
            "Item review: {}  |  1 Front  2 Side  3 Back  |  F12 Screenshot",
            settings.focus.as_deref().unwrap_or("all seeds")
        )
        }),
        Node {
            position_type: PositionType::Absolute,
            top: px(18),
            left: px(18),
            ..default()
        },
    ));
}

fn review(
    mut commands: Commands,
    settings: Res<Settings>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut captured: Local<bool>,
    mut props: Query<&mut Transform, With<Prop>>,
) {
    if settings.benchmark {
        return;
    }
    for (key, degrees) in [
        (KeyCode::Digit1, 0.0_f32),
        (KeyCode::Digit2, 90.0),
        (KeyCode::Digit3, 180.0),
    ] {
        if keys.just_pressed(key) {
            for mut transform in &mut props {
                transform.rotation = Quat::from_rotation_y(degrees.to_radians());
            }
        }
    }
    let path = if keys.just_pressed(KeyCode::F12) {
        Some("item-review.png".into())
    } else if !*captured && time.elapsed_secs() >= 2.0 {
        *captured = true;
        settings.screenshot.clone()
    } else {
        None
    };
    if let Some(path) = path {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
}

fn benchmark_frames(
    settings: Res<Settings>,
    time: Res<Time<Real>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut samples: Local<Vec<f64>>,
    mut exit: MessageWriter<AppExit>,
) {
    if !settings.benchmark || time.elapsed_secs_f64() < 2.0 {
        return;
    }
    samples.push(time.delta_secs_f64() * 1_000.0);
    if samples.len() == 600 {
        samples.sort_unstable_by(f64::total_cmp);
        let window = windows.single().expect("item review window");
        println!(
            "TK ITEM REVIEW frames={} physical={}x{} median_ms={:.3} p95_ms={:.3}",
            samples.len(),
            window.physical_width(),
            window.physical_height(),
            (samples[299] + samples[300]) * 0.5,
            samples[569],
        );
        exit.write(AppExit::Success);
    }
}
