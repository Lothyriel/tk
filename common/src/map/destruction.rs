//! Sparse shared solid geometry. Authority and presentation subtract the same cuts.
use bevy::prelude::*;
use bevy_rapier3d::prelude::Collider;
use rkyv::{Archive, Deserialize, Serialize};

use super::{BarrierCell, DestroyedCells};
use crate::{combat::SurfaceKind, raid::RaidPhase, WeaponKind};

pub const CHANNEL: u8 = 5;
pub const MAX_CUTS: usize = 128;
pub const MAX_SOLIDS: usize = 96;
pub const MAX_RUBBLE: usize = 64;
pub const RUBBLE_LIFETIME: f32 = 20.0;

#[derive(Debug, Clone, PartialEq, Archive, Serialize, Deserialize)]
pub struct Cut {
    pub cell: u16,
    /// Coordinates relative to the original structural cell.
    pub center: [f32; 3],
    pub half_size: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Archive, Serialize, Deserialize)]
pub struct RubblePose {
    pub cell: u16,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: f32,
}

#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
pub struct CoverSnapshot {
    pub sequence: u64,
    pub raid_id: u32,
    pub phase: RaidPhase,
    pub removed: DestroyedCells,
    pub cuts: Vec<Cut>,
    pub rubble: Vec<RubblePose>,
}

#[derive(Debug, Clone, Copy)]
pub struct SolidBox {
    pub center: Vec3,
    pub half_size: Vec3,
}

pub fn bore_width(weapon: WeaponKind) -> f32 {
    match weapon {
        WeaponKind::Rifle => 0.006,
        WeaponKind::Smg => 0.009,
        WeaponKind::Pistol => 0.011,
    }
}

pub fn impact_cut(cell: &BarrierCell, point: Vec3, normal: Vec3, weapon: WeaponKind) -> Cut {
    let axis = (0..3)
        .max_by(|&a, &b| normal[a].abs().total_cmp(&normal[b].abs()))
        .unwrap();
    let width = bore_width(weapon);
    let mut center = point - Vec3::from(cell.center);
    let mut half_size = Vec3::splat(width * 0.5);
    if matches!(cell.surface, SurfaceKind::Wood) {
        center[axis] = 0.0;
        half_size[axis] = cell.half_size[axis] + 0.002;
    } else {
        // A shallow chip has a wider mouth but retains concrete behind it.
        half_size = Vec3::splat(width * 1.5);
        half_size[axis] = 0.008;
        center -= normal * 0.007;
    }
    Cut {
        cell: cell.id,
        center: center.into(),
        half_size: half_size.into(),
    }
}

/// Exact disjoint box subtraction; no uniform millimetre voxel grid is allocated.
pub fn subtract(solid: SolidBox, cut: &Cut) -> Vec<SolidBox> {
    let mut lo = solid.center - solid.half_size;
    let mut hi = solid.center + solid.half_size;
    let c = Vec3::from(cut.center);
    let h = Vec3::from(cut.half_size);
    let cut_lo = lo.max(c - h);
    let cut_hi = hi.min(c + h);
    if (cut_hi - cut_lo).min_element() <= 0.0 {
        return vec![solid];
    }
    let mut result = Vec::new();
    for axis in 0..3 {
        if cut_lo[axis] > lo[axis] {
            let mut end = hi;
            end[axis] = cut_lo[axis];
            result.push(SolidBox {
                center: (lo + end) * 0.5,
                half_size: (end - lo) * 0.5,
            });
            lo[axis] = cut_lo[axis];
        }
        if cut_hi[axis] < hi[axis] {
            let mut start = lo;
            start[axis] = cut_hi[axis];
            result.push(SolidBox {
                center: (start + hi) * 0.5,
                half_size: (hi - start) * 0.5,
            });
            hi[axis] = cut_hi[axis];
        }
    }
    result
}

pub fn solids(cell: &BarrierCell, cuts: &[Cut]) -> Vec<SolidBox> {
    let mut result = vec![SolidBox {
        center: Vec3::ZERO,
        half_size: cell.half_size.into(),
    }];
    for cut in cuts.iter().filter(|cut| cut.cell == cell.id) {
        result = result
            .into_iter()
            .flat_map(|solid| subtract(solid, cut))
            .collect();
    }
    result
}

pub fn collider(solids: &[SolidBox]) -> Collider {
    Collider::compound(
        solids
            .iter()
            .map(|solid| {
                (
                    solid.center,
                    Quat::IDENTITY,
                    Collider::cuboid(solid.half_size.x, solid.half_size.y, solid.half_size.z),
                )
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destruction_bores_have_nominal_width_and_adjacent_wood_remains_solid() {
        let cell = super::super::barrier_cells()
            .into_iter()
            .find(|c| matches!(c.surface, SurfaceKind::Wood))
            .unwrap();
        for (weapon, expected) in [
            (WeaponKind::Rifle, 0.006),
            (WeaponKind::Smg, 0.009),
            (WeaponKind::Pistol, 0.011),
        ] {
            let center = Vec3::from(cell.center);
            let cut = impact_cut(&cell, center + Vec3::Z * cell.half_size[2], Vec3::Z, weapon);
            assert_eq!(cut.half_size[0] * 2.0, expected);
            assert_eq!(cut.half_size[1] * 2.0, expected);
            let pieces = solids(&cell, &[cut]);
            let shape = collider(&pieces);
            assert!(shape
                .cast_ray(
                    center,
                    Quat::IDENTITY,
                    center + Vec3::Z,
                    -Vec3::Z,
                    2.0,
                    true
                )
                .is_none());
            assert!(shape
                .cast_ray(
                    center,
                    Quat::IDENTITY,
                    center + Vec3::Z + Vec3::X * 0.03,
                    -Vec3::Z,
                    2.0,
                    true
                )
                .is_some());
            let remaining: f32 = pieces
                .iter()
                .map(|b| b.half_size.element_product() * 8.0)
                .sum();
            let original = Vec3::from(cell.half_size).element_product() * 8.0;
            assert!(
                (original - remaining - expected * expected * cell.half_size[2] * 2.0).abs()
                    < 0.000001
            );
        }
    }

    #[test]
    fn destruction_concrete_chip_retains_a_solid_back_and_clips_at_cell_edges() {
        let cell = super::super::barrier_cells().remove(0);
        let center = Vec3::from(cell.center);
        let cut = impact_cut(
            &cell,
            center + Vec3::Z * cell.half_size[2],
            Vec3::Z,
            WeaponKind::Rifle,
        );
        let pieces = solids(&cell, std::slice::from_ref(&cut));
        let shape = collider(&pieces);
        let hit = shape
            .cast_ray(
                center,
                Quat::IDENTITY,
                center + Vec3::Z,
                -Vec3::Z,
                2.0,
                true,
            )
            .unwrap();
        assert!((hit - (1.0 - cell.half_size[2] + 0.015)).abs() < 0.00001);
        for b in pieces {
            assert!(b.half_size.min_element() > 0.0);
            assert!((b.center.abs() + b.half_size)
                .cmple(Vec3::from(cell.half_size) + Vec3::splat(0.000001))
                .all());
        }
        let outside = Cut {
            center: [100.0; 3],
            ..cut
        };
        assert_eq!(solids(&cell, &[outside]).len(), 1);
    }

    #[test]
    fn destruction_wire_state_roundtrips_within_its_own_budget() {
        let snapshot = CoverSnapshot {
            sequence: 77,
            raid_id: 5,
            phase: RaidPhase::Active,
            removed: super::super::destroyed_mask(&[1, 72]),
            cuts: (0..MAX_CUTS)
                .map(|n| Cut {
                    cell: n as u16,
                    center: [0.01, 0.0, 0.0],
                    half_size: [0.003, 0.003, 0.3],
                })
                .collect(),
            rubble: (0..MAX_RUBBLE)
                .map(|n| RubblePose {
                    cell: n as u16,
                    position: [1.0, 0.2, 3.0],
                    rotation: Quat::IDENTITY.to_array(),
                    scale: 1.0,
                })
                .collect(),
        };
        let bytes = crate::data::encode(&snapshot);
        assert!(bytes.len() < 16_000);
        let decoded = crate::data::decode::<CoverSnapshot>(&bytes);
        assert_eq!(decoded.sequence, 77);
        assert_eq!(decoded.raid_id, 5);
        assert_eq!(decoded.phase, RaidPhase::Active);
        assert_eq!(decoded.removed, snapshot.removed);
        assert_eq!(decoded.cuts, snapshot.cuts);
        assert_eq!(decoded.rubble, snapshot.rubble);
    }
}
