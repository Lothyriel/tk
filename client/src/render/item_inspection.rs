//! Local presentation of an item from the confirmed private inventory.
use bevy::{camera::visibility::RenderLayers, light::NotShadowCaster, prelude::*};
use common::{
    PlayerId,
    catalog::{
        ItemInstanceId,
        item_manifest::{ItemVisual, item_visuals},
    },
    combat::Region,
    humanoid::{
        PhysicalMode,
        handling::{self, ArmSolution, Hand},
    },
};

use super::{
    VIEW_MODEL_RENDER_LAYER, ViewModelCamera,
    voxel::{VoxelModel, configured::HandAssets},
    weapons::grasp,
};
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
pub(crate) struct Preview {
    pub(crate) raid: u32,
    pub(crate) item: ItemInstanceId,
    pub(crate) yaw_steps: i8,
    asset: String,
    pub(crate) model: Entity,
    arm: [Entity; 3],
}

fn present(
    mut commands: Commands,
    (session, id, panel, inventory): (Res<ClientSession>, Res<PlayerId>, Res<Panel>, Res<State>),
    camera: Single<(Entity, &Projection), With<ViewModelCamera>>,
    mut previews: Query<(Entity, &mut Preview)>,
    (mut hands, mut meshes, mut materials): (
        ResMut<HandAssets>,
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
    ),
) {
    let raid = session.raid.raid_id;
    let active = panel.inspection.and_then(|inspect| {
        let local = session.local(id.0)?;
        if !panel.open
            || !session.playing(id.0)
            || !matches!(
                local.pose.mode,
                PhysicalMode::Standing | PhysicalMode::Stumbling
            )
        {
            return None;
        }
        let anatomy = &local.body.anatomy;
        let side = if anatomy.capacity(Region::LeftArm) > 0.0 {
            Hand::Support
        } else if anatomy.capacity(Region::RightArm) > 0.0 {
            Hand::Trigger
        } else {
            return None;
        };
        let item = inventory
            .owned_grid(raid, id.0)?
            .items
            .iter()
            .find(|(id, _)| *id == inspect.item)?;
        Some((inspect, item_visuals().get(&item.1.definition)?, side))
    });
    let aspect = match camera.1 {
        Projection::Perspective(lens) => lens.aspect_ratio,
        _ => 1.0,
    };
    let active = active.and_then(|(inspect, visual, side)| {
        Some((inspect, visual, held_pose(inspect, visual, aspect, side)?))
    });
    let mut reused = false;
    for (entity, mut preview) in &mut previews {
        if !reused
            && let Some((inspect, visual, (model, arm))) = active
            && preview.raid == raid
            && preview.item == inspect.item
            && preview.asset == visual.asset_key
        {
            preview.yaw_steps = inspect.yaw_steps;
            commands.entity(preview.model).insert(model);
            for (entity, pose) in preview.arm.into_iter().zip(arm) {
                commands.entity(entity).insert(pose);
            }
            reused = true;
        } else {
            commands.entity(entity).try_despawn();
        }
    }
    if !reused && let Some((inspect, visual, (model, arm))) = active {
        let root = commands
            .spawn((
                Transform::default(),
                Visibility::Inherited,
                ChildOf(camera.0),
            ))
            .id();
        let mesh = commands
            .spawn((
                VoxelModel {
                    asset: visual.asset_key.clone(),
                    view_model: true,
                },
                model,
                Visibility::Inherited,
                ChildOf(root),
            ))
            .id();
        let handles = hands.load(&mut meshes, &mut materials);
        let arm = std::array::from_fn(|index| {
            let handle = [&handles.0, &handles.1, &handles.2][index];
            let mut entity = commands.spawn((
                Mesh3d(handle.clone()),
                MeshMaterial3d(handles.3.clone()),
                arm[index],
                Visibility::Inherited,
                RenderLayers::layer(VIEW_MODEL_RENDER_LAYER),
                NotShadowCaster,
                ChildOf(root),
            ));
            if index == 2 {
                entity.insert((grasp::component([0.0; 2]), grasp::bounds()));
            }
            entity.id()
        });
        commands.entity(root).insert(Preview {
            raid,
            item: inspect.item,
            yaw_steps: inspect.yaw_steps,
            asset: visual.asset_key.clone(),
            model: mesh,
            arm,
        });
    }
}

fn held_pose(
    inspect: Inspection,
    visual: &ItemVisual,
    aspect: f32,
    side: Hand,
) -> Option<(Transform, [Transform; 3])> {
    let angle = std::f32::consts::FRAC_PI_6;
    let half = Vec3::from_array(visual.collider_half_extents());
    // Flat objects are tilted toward the eye so their faces are readable.
    let tilt = if half.y < 0.018 { 1.25 } else { 0.5 };
    let rotation = Quat::from_euler(
        EulerRot::YXZ,
        f32::from(inspect.yaw_steps) * angle,
        (f32::from(inspect.pitch_steps) + tilt) * angle,
        0.0,
    );
    let center = Vec3::from_array(visual.collider_center());
    let radius = half.length();
    let depth = (0.28 + 0.95 * radius) / aspect.clamp(0.76, 1.0);
    let direction = rotation.inverse() * Vec3::NEG_Y;
    // Approximate the supporting face using the authored bounds. At grazing
    // angles ease across that face rather than snapping between its corners.
    let support = rotation * (half * direction.map(|c| (c / 0.12).clamp(-1.0, 1.0)));
    let handed = if side == Hand::Support { -1.0 } else { 1.0 };
    let glove = Quat::from_rotation_y(-handed * 0.28) * Quat::from_rotation_z(std::f32::consts::PI);
    // This point is on the upper surface of the fingers in the shared glove
    // mesh after turning its palm up. Keep the entire glove below the item.
    let wrist_offset = Vec3::new(support.x, -half.dot(direction.abs()), support.z)
        - glove * Vec3::new(0.0, -0.044, -0.037);
    let shoulder = Vec3::new(handed * 0.20, -0.30, -0.015);
    let planar = (Vec3::new(0.0, -0.025, 0.0) + wrist_offset - shoulder).xy();
    let max_depth =
        wrist_offset.z - shoulder.z + (0.57_f32.powi(2) - planar.length_squared()).max(0.0).sqrt();
    let near = 0.11 + radius;
    let far = max_depth.min(0.57);
    if near >= far {
        return None;
    }
    // Fit the neutral depth to the reachable interval before applying zoom.
    // That keeps the controls useful even when the neutral framing is limited.
    let margin = (far - near) * 0.22;
    let depth =
        depth.clamp(near + margin, far - margin) - margin * 0.5 * f32::from(inspect.zoom_steps);
    let shown = Vec3::new(0.0, -0.025, -depth);
    let ArmSolution { elbow, wrist } = handling::solve_arm(
        side,
        shoulder,
        Transform::from_translation(shown + wrist_offset).with_rotation(glove),
        Vec3::new(handed * 0.8, -1.0, 0.1),
    )
    .ok()?;
    let arm = ArmSolution { elbow, wrist }
        .segments(shoulder)
        .map(|pose| pose.transform());
    Some((
        Transform::from_translation(shown - rotation * center).with_rotation(rotation),
        arm,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::catalog::item_manifest::S1_SEEDS;

    #[test]
    fn item_inspection_holds_all_seed_models_with_rigid_support_and_near_clearance() {
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
            let (pose, _) = held_pose(inspect, visual, 0.7, Hand::Support).unwrap();
            let displayed = pose.transform_point(center);
            assert!((displayed.y + 0.025).abs() < 1e-6);
            assert!(-displayed.z - radius >= 0.1);
            assert_eq!(pose.scale, Vec3::ONE);
            let (normal, _) = held_pose(
                Inspection {
                    zoom_steps: 0,
                    ..inspect
                },
                visual,
                0.7,
                Hand::Support,
            )
            .unwrap();
            assert!(normal.transform_point(center).z < displayed.z);
            for yaw in 0..12 {
                for pitch in -2..=2 {
                    for zoom in -2..=2 {
                        for side in [Hand::Support, Hand::Trigger] {
                            let inspect = Inspection {
                                yaw_steps: yaw,
                                pitch_steps: pitch,
                                zoom_steps: zoom,
                                ..inspect
                            };
                            let (item, [upper, lower, glove]) =
                                held_pose(inspect, visual, 0.76, side)
                                    .unwrap_or_else(|| panic!("{seed}: {inspect:?} {side:?}"));
                            assert_eq!(item.scale, Vec3::ONE);
                            assert!(-item.transform_point(center).z - radius >= 0.1);
                            let bottom = item.transform_point(center).y
                                - Vec3::from_array(visual.collider_half_extents())
                                    .dot((item.rotation.inverse() * Vec3::Y).abs());
                            let palm = glove.transform_point(Vec3::new(0.0, -0.044, -0.037));
                            assert!((palm.y - bottom).abs() < 1e-5);
                            let end = |part: Transform, length| {
                                let h = part.rotation * Vec3::Y * (length * 0.5);
                                [part.translation + h, part.translation - h]
                            };
                            let [a, b] = end(lower, handling::FOREARM_METRES);
                            assert!(
                                a.distance(glove.translation)
                                    .min(b.distance(glove.translation))
                                    < 1e-5
                            );
                            assert!(
                                end(upper, handling::UPPER_ARM_METRES)
                                    .into_iter()
                                    .any(|p| p.distance(a).min(p.distance(b)) < 1e-5)
                            );
                        }
                    }
                }
            }
        }
    }
}
