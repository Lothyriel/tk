//! Geometric targeting for static world items, independent of ECS allocation.
use bevy::prelude::{Aabb3d, Dir3, RayCast3d, Vec3};

use crate::catalog::ItemInstanceId;

pub const PICKUP_REACH_M: f32 = 2.25;

#[derive(Clone, Copy)]
pub struct GroundTarget {
    pub id: ItemInstanceId,
    pub center: Vec3,
    pub half_extents: Vec3,
}

/// `obstruction` is the nearest solid hit on the same ray, as measured by
/// the server. A coincident solid wins rather than permitting pickup through it.
pub fn first_ground_target(
    origin: Vec3,
    direction: Dir3,
    obstruction: Option<f32>,
    targets: impl IntoIterator<Item = GroundTarget>,
) -> Option<ItemInstanceId> {
    if !origin.is_finite() || obstruction.is_some_and(|d| !d.is_finite() || d <= 0.0) {
        return None;
    }
    let ray = RayCast3d::new(origin, direction, PICKUP_REACH_M);
    targets
        .into_iter()
        .filter(|target| {
            target.center.is_finite()
                && target.half_extents.is_finite()
                && target.half_extents.min_element() > 0.0
        })
        .filter_map(|target| {
            let distance =
                ray.aabb_intersection_at(&Aabb3d::new(target.center, target.half_extents))?;
            obstruction
                .is_none_or(|solid| distance < solid)
                .then_some((distance, target.id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)))
        .map(|(_, id)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ground_target_respects_reach_occlusion_and_stable_ties() {
        let target = |id, x, z| GroundTarget {
            id: ItemInstanceId(id),
            center: Vec3::new(x, 0.0, z),
            half_extents: Vec3::splat(0.125),
        };
        let near = target(2, 0.0, -1.0);
        let far = target(1, 0.0, -2.0);
        let misses = [
            target(3, 0.5, -0.5),
            target(4, 0.0, -3.0),
            target(5, 0.0, 1.0),
        ];
        let pick = |solid, targets: &[GroundTarget]| {
            first_ground_target(Vec3::ZERO, Dir3::NEG_Z, solid, targets.iter().copied())
        };
        assert_eq!(pick(None, &misses), None);
        assert_eq!(pick(None, &[far, near]), Some(near.id));
        assert_eq!(pick(Some(0.875), &[near]), None);
        assert_eq!(pick(Some(1.5), &[far, near]), Some(near.id));
        let tie = target(1, 0.0, -1.0);
        assert_eq!(pick(None, &[near, tie]), Some(tie.id));
        assert_eq!(pick(None, &[tie, near]), Some(tie.id));
        assert_eq!(pick(Some(f32::NAN), &[near]), None);
    }
}
