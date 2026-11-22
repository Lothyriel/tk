//! Local presentation of an item from the confirmed private inventory.
use bevy::prelude::*;
use common::{
    PlayerId,
    catalog::{ItemInstanceId, item_manifest::item_visuals},
};

use super::{ViewModelCamera, voxel::VoxelModel};
use crate::{
    ClientSystems,
    input::inventory_panel::{Inspection, Panel},
    sync::{ClientSession, inventory::State},
};

pub(super) struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, present.in_set(ClientSystems::Present));
    }
}

#[derive(Component)]
struct Preview {
    raid: u32,
    item: ItemInstanceId,
}

fn present(
    mut commands: Commands,
    (session, id, panel, inventory): (Res<ClientSession>, Res<PlayerId>, Res<Panel>, Res<State>),
    camera: Single<(Entity, &Projection), With<ViewModelCamera>>,
    mut previews: Query<(Entity, &Preview, &VoxelModel, &mut Transform)>,
) {
    let raid = session.raid.raid_id;
    let active = panel.inspection.and_then(|inspect| {
        if !panel.open || !session.playing(id.0) {
            return None;
        }
        let item = inventory
            .owned_grid(raid, id.0)?
            .items
            .iter()
            .find(|(id, _)| *id == inspect.item)?;
        Some((inspect, item_visuals().get(&item.1.definition)?))
    });
    let aspect = match camera.1 {
        Projection::Perspective(lens) => lens.aspect_ratio,
        _ => 1.0,
    };
    let mut reused = false;
    for (entity, preview, model, mut transform) in &mut previews {
        if !reused
            && let Some((inspect, visual)) = active
            && preview.raid == raid
            && preview.item == inspect.item
            && model.asset == visual.asset_key
        {
            *transform = preview_pose(inspect, visual, aspect);
            reused = true;
        } else {
            commands.entity(entity).try_despawn();
        }
    }
    if !reused && let Some((inspect, visual)) = active {
        commands.spawn((
            Preview {
                raid,
                item: inspect.item,
            },
            VoxelModel {
                asset: visual.asset_key.clone(),
                view_model: true,
            },
            preview_pose(inspect, visual, aspect),
            Visibility::Inherited,
            ChildOf(camera.0),
        ));
    }
}

fn preview_pose(
    inspect: Inspection,
    visual: &common::catalog::item_manifest::ItemVisual,
    aspect: f32,
) -> Transform {
    let angle = std::f32::consts::FRAC_PI_6;
    let rotation = Quat::from_euler(
        EulerRot::YXZ,
        f32::from(inspect.yaw_steps) * angle,
        (f32::from(inspect.pitch_steps) + 0.5) * angle,
        0.0,
    );
    // Preserve metres and rotate about the authored bounds centre. Use a
    // bounding sphere so every angle fits even in a narrower split window.
    let radius = Vec3::from_array(visual.collider_half_extents()).length();
    let depth = (radius * (3.6 - 0.35 * f32::from(inspect.zoom_steps)) / aspect.clamp(0.35, 1.0))
        .max(0.11 + radius);
    Transform::from_translation(
        Vec3::NEG_Z * depth - rotation * Vec3::from_array(visual.collider_center()),
    )
    .with_rotation(rotation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::catalog::item_manifest::S1_SEEDS;

    #[test]
    fn item_inspection_rotates_about_authored_center_without_scaling() {
        for seed in S1_SEEDS {
            let visual = &item_visuals()[seed];
            let center = Vec3::from_array(visual.collider_center());
            let radius = Vec3::from_array(visual.collider_half_extents()).length();
            let inspect = Inspection {
                item: ItemInstanceId(1),
                yaw_steps: 6,
                pitch_steps: 2,
                zoom_steps: 2,
            };
            let pose = preview_pose(inspect, visual, 0.7);
            let displayed = pose.transform_point(center);
            assert!(displayed.xy().length() < 1e-6);
            assert!(-displayed.z - radius >= 0.1);
            assert_eq!(pose.scale, Vec3::ONE);
            let normal = preview_pose(
                Inspection {
                    zoom_steps: 0,
                    ..inspect
                },
                visual,
                0.7,
            );
            assert!(normal.transform_point(center).z < displayed.z);
        }
    }
}
