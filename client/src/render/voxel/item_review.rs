//! Standalone rendered review. Uses the production voxel and icon caches.
use super::{VoxelCache, VoxelModel, instantiate};
use crate::render::item_icons::{self, ItemIcon};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use common::catalog::item_manifest::{S1_SEEDS, item_visuals};

pub(crate) struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VoxelCache>()
            .add_plugins(item_icons::Plugin)
            .add_systems(Startup, setup)
            .add_systems(Update, (instantiate, review));
    }
}

#[derive(Component)]
struct Prop;

fn setup(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 0.85, -1.65).looking_at(Vec3::new(0.0, 0.08, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 9000.0,
            ..default()
        },
        Transform::from_xyz(1.0, 2.0, -1.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 500.0,
        ..default()
    });
    for (index, id) in S1_SEEDS.into_iter().enumerate() {
        let binding = &item_visuals()[id];
        let center = Vec3::from_array(binding.collider_center());
        let pad = Vec3::new((index as f32 - 2.5) * -0.21, 0.0, 0.0);
        let offset = Vec3::new(center.x, binding.min[1], center.z);
        commands
            .spawn((
                Transform::from_translation(pad),
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
        Text::new("Item review  |  1 Front  2 Side  3 Back  |  F12 Screenshot"),
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
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut captured: Local<bool>,
    mut props: Query<&mut Transform, With<Prop>>,
) {
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
        std::env::var_os("TK_ITEM_REVIEW_SCREENSHOT").map(std::path::PathBuf::from)
    } else {
        None
    };
    if let Some(path) = path {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
}
