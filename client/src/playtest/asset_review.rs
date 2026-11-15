//! Four static views of the accepted remote assembly in the live game scene.
//! Only the diagnostic camera moves; actor input, meshes, lighting and projection
//! stay on the ordinary path. This is asset evidence, not a player inspect action.
use super::*;
use crate::render::{
    PlayerBodyVisual, WorldModelCamera,
    voxel::{
        VoxelPart,
        configured::{ConfiguredPose, ConfiguredWeapon},
    },
};

pub(super) const VIEWS: [&str; 4] = ["right", "left", "front", "top"];

/// Remove body occlusion for dimensional views of the live assembly. Equipped
/// actor/contact evidence is captured in the preceding action route.
pub(super) fn body_visibility(
    driver: Res<Driver>,
    bodies: Query<(), With<PlayerBodyVisual>>,
    parts: Query<(), With<VoxelPart>>,
    parents: Query<&ChildOf>,
    mut meshes: Query<(Entity, &mut Visibility), With<Mesh3d>>,
    mut saved: Local<std::collections::HashMap<Entity, Visibility>>,
) {
    if driver.step != Step::AssetReview {
        for (entity, visibility) in saved.drain() {
            if let Ok((_, mut current)) = meshes.get_mut(entity) {
                *current = visibility;
            }
        }
        return;
    }
    for (entity, mut visibility) in &mut meshes {
        let mut parent = entity;
        while let Ok(next) = parents.get(parent) {
            parent = next.0;
            if parts.contains(parent) {
                break;
            }
            if bodies.contains(parent) {
                saved.entry(entity).or_insert(*visibility);
                *visibility = Visibility::Hidden;
                break;
            }
        }
    }
}

pub(super) fn camera(
    driver: Res<Driver>,
    time: Res<Time<Real>>,
    id: Res<PlayerId>,
    session: Res<ClientSession>,
    weapons: Query<
        (
            &ConfiguredWeapon,
            &ConfiguredPose,
            &GlobalTransform,
            &InheritedVisibility,
        ),
        Without<WorldModelCamera>,
    >,
    mut camera: Single<&mut GlobalTransform, With<WorldModelCamera>>,
) {
    if driver.step != Step::AssetReview {
        return;
    }
    for (request, pose, global, visible) in &weapons {
        if request.view_model || !visible.get() {
            continue;
        }
        let Some(entry) = session
            .configurations
            .cache
            .get(request.epoch, request.handle)
        else {
            continue;
        };
        if entry.packet.owner == id.0
            || !session
                .players
                .iter()
                .any(|p| p.id == entry.packet.owner && !p.bot)
        {
            continue;
        }
        let index = (((time.elapsed_secs_f64() - driver.since) / 2.0) as usize).min(3);
        let root = global.mul_transform(pose.solution.root);
        let center = root.transform_point(Vec3::new(0.0, -0.025, 0.02));
        let (offset, up) = match index {
            0 => (Vec3::X * 1.05, Vec3::Y),
            1 => (Vec3::NEG_X * 1.05, Vec3::Y),
            2 => (Vec3::NEG_Z * 1.05, Vec3::Y),
            _ => (Vec3::Y * 1.05, Vec3::NEG_Z),
        };
        let rotation = root.rotation();
        **camera = Transform::from_translation(center + rotation * offset)
            .looking_at(center, rotation * up)
            .into();
        break;
    }
}
