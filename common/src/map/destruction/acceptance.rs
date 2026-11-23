use super::*;

fn carved(bounds: SolidBox, cutters: &[SolidBox]) -> Vec<SolidBox> {
    let mut remaining = vec![bounds];
    for cutter in cutters {
        let cut = Cut {
            cell: 0,
            center: cutter.center.into(),
            half_size: cutter.half_size.into(),
        };
        remaining = remaining
            .into_iter()
            .flat_map(|b| subtract(b, &cut))
            .collect();
    }
    remaining
}

fn impact(direction: Vec3) -> material::Impact {
    material::Impact {
        point: Vec3::Z * 0.12,
        normal: Vec3::Z,
        entry: Vec3::Z * 0.12,
        entry_normal: Vec3::Z,
        direction,
        diameter: 0.006,
        damage: 34.0,
        penetration: 36.0,
        dose: 34.0,
        seed: 7,
    }
}

#[test]
fn destruction_oblique_bore_follows_ray_without_a_wide_rectangular_gash() {
    let bounds = SolidBox {
        center: Vec3::ZERO,
        half_size: Vec3::new(0.5, 0.5, 0.12),
    };
    for direction in [Vec3::NEG_Z, Vec3::new(0.6, 0.0, -0.8)] {
        let hit = impact(direction);
        let cuts = material::cutters(bounds, SurfaceKind::Wood, hit);
        let remaining = carved(bounds, &cuts);
        let lost = bounds.volume() - remaining.iter().map(|b| b.volume()).sum::<f32>();
        let path_length = 0.24 / direction.z.abs();
        let nominal = 0.006_f32.powi(2) * path_length;
        assert!(
            lost > nominal * 0.85 && lost < nominal * 1.8,
            "oblique lost={lost}, nominal={nominal}"
        );
        let shape = collider(&remaining);
        assert!(shape
            .cast_ray(
                Vec3::ZERO,
                Quat::IDENTITY,
                hit.point - direction * 0.02,
                direction,
                0.6,
                true
            )
            .is_none());
        assert!(shape
            .cast_ray(
                Vec3::ZERO,
                Quat::IDENTITY,
                hit.point - direction * 0.02 + Vec3::X * 0.03,
                direction,
                0.6,
                true
            )
            .is_some());
    }
}

#[test]
fn destruction_material_degradation_and_attenuation_change_real_local_geometry() {
    let bounds = SolidBox {
        center: Vec3::ZERO,
        half_size: Vec3::new(0.5, 0.5, 0.12),
    };
    let mut hit = impact(Vec3::NEG_Z);
    let fresh_wood = carved(bounds, &material::cutters(bounds, SurfaceKind::Wood, hit));
    let fresh_concrete = carved(
        bounds,
        &material::cutters(bounds, SurfaceKind::Concrete, hit),
    );
    let loss = |remaining: &[SolidBox]| {
        bounds.volume() - remaining.iter().map(|b| b.volume()).sum::<f32>()
    };
    assert!(collider(&fresh_wood)
        .cast_ray(Vec3::ZERO, Quat::IDENTITY, Vec3::Z, Vec3::NEG_Z, 2.0, true)
        .is_none());
    assert!(collider(&fresh_concrete)
        .cast_ray(Vec3::ZERO, Quat::IDENTITY, Vec3::Z, Vec3::NEG_Z, 2.0, true)
        .is_some());
    hit.dose = 250.0;
    let degraded = carved(bounds, &material::cutters(bounds, SurfaceKind::Wood, hit));
    assert!(loss(&degraded) > loss(&fresh_wood) * 10.0);
    hit.dose = 34.0;
    hit.damage = 3.4;
    let attenuated = carved(
        bounds,
        &material::cutters(bounds, SurfaceKind::Concrete, hit),
    );
    assert!(loss(&attenuated) < loss(&fresh_concrete) * 0.5);
    hit.penetration = 0.5;
    assert!(collider(&carved(
        bounds,
        &material::cutters(bounds, SurfaceKind::Wood, hit)
    ))
    .cast_ray(Vec3::ZERO, Quat::IDENTITY, Vec3::Z, Vec3::NEG_Z, 2.0, true)
    .is_some());
}

#[test]
fn destruction_material_fines_are_size_based_not_a_body_budget_fallback() {
    let dust = SolidBox {
        center: Vec3::ZERO,
        half_size: Vec3::splat(0.002),
    };
    let chip = SolidBox {
        center: Vec3::ZERO,
        half_size: Vec3::splat(0.015),
    };
    let thin_sheet = SolidBox {
        center: Vec3::ZERO,
        half_size: Vec3::new(0.5, 0.5, 0.0001),
    };
    for surface in [SurfaceKind::Wood, SurfaceKind::Concrete] {
        assert!(!material::physical_debris(surface, dust));
        assert!(material::physical_debris(surface, chip));
        assert!(
            material::physical_debris(surface, thin_sheet),
            "substantial thin material must not vanish as dust"
        );
    }
}

#[test]
fn destruction_progressive_concrete_rim_opens_recorded_entry_without_removing_backstop() {
    let bounds = SolidBox {
        center: Vec3::ZERO,
        half_size: Vec3::new(0.5, 0.5, 0.12),
    };
    let mut hit = impact(Vec3::NEG_Z);
    hit.point = Vec3::new(0.009, 0.0, 0.08);
    hit.dose = 170.0;
    // Later contact can be a cavity SIDE. The rough mouth and chip escape
    // still belong to the first exterior contact plane at z=0.12.
    hit.normal = Vec3::X;
    let remaining = carved(
        bounds,
        &material::cutters(bounds, SurfaceKind::Concrete, hit),
    );
    let shape = collider(&remaining);
    assert!(
        shape
            .cast_ray(Vec3::ZERO, Quat::IDENTITY, hit.point, Vec3::Z, 0.10, true)
            .is_none(),
        "a wider recessed pocket must open its original local mouth"
    );
    assert!(
        shape
            .cast_ray(
                Vec3::ZERO,
                Quat::IDENTITY,
                Vec3::new(0.15, 0.0, 0.3),
                Vec3::NEG_Z,
                1.0,
                true
            )
            .is_some(),
        "neighboring healthy concrete must remain"
    );
    assert!(
        shape
            .cast_ray(
                Vec3::ZERO,
                Quat::IDENTITY,
                hit.point,
                Vec3::NEG_Z,
                1.0,
                true
            )
            .is_some(),
        "opening the entry rim must not perforate the remaining backstop"
    );
}

#[test]
fn destruction_remaining_ligament_area_and_internal_islands_determine_support() {
    let node = |center, half_size| support::Node {
        cell: 72,
        structure: 6,
        surface: SurfaceKind::Wood,
        solid: SolidBox { center, half_size },
    };
    let upper = node(Vec3::new(0.0, 0.65, 0.0), Vec3::new(0.25, 0.5, 0.12));
    let strong = node(Vec3::new(0.0, 0.125, 0.0), Vec3::new(0.25, 0.025, 0.12));
    let weak = node(Vec3::new(0.0, 0.125, 0.0), Vec3::new(0.0003, 0.025, 0.12));
    assert_eq!(support::unsupported(&[strong, upper]), [false, false]);
    assert_eq!(support::unsupported(&[weak, upper]), [false, true]);
    let island = node(Vec3::new(0.4, 0.4, 0.0), Vec3::splat(0.02));
    assert_eq!(
        support::unsupported(&[strong, upper, island]),
        [false, false, true]
    );
}

#[test]
fn destruction_irregular_partition_conserves_volume_and_crosses_cell_seams() {
    let mut nodes = Vec::new();
    for (cell, x) in [(72, -0.25), (76, 0.25)] {
        let original = SolidBox {
            center: Vec3::new(x, 1.0, 0.0),
            half_size: Vec3::new(0.25, 0.25, 0.12),
        };
        let split = material::fracture_boxes(original, cell as u32);
        assert!((split.iter().map(|b| b.volume()).sum::<f32>() - original.volume()).abs() < 1e-6);
        nodes.extend(split.into_iter().map(|solid| support::Node {
            cell,
            structure: 6,
            surface: SurfaceKind::Wood,
            solid,
        }));
    }
    let groups = support::fragments(&nodes);
    assert!(groups
        .iter()
        .any(|g| g.iter().any(|&i| nodes[i].cell == 72) && g.iter().any(|&i| nodes[i].cell == 76)));
    assert_eq!(groups.iter().map(Vec::len).sum::<usize>(), nodes.len());
    assert!(groups.iter().all(|g| g.len() <= MAX_FRAGMENT_SOLIDS));
}

#[test]
fn destruction_full_frozen_shape_envelope_fits_wire_budget_and_rejects_invalid_shapes() {
    let cells = super::super::barrier_cells();
    let mut snapshot = CoverSnapshot {
        sequence: 9,
        raid_id: 1,
        phase: RaidPhase::Active,
        removed: super::super::destroyed_mask(&[]),
        cuts: cells
            .iter()
            .take(MAX_CUTS)
            .map(|cell| Cut {
                cell: cell.id,
                center: [0.0; 3],
                half_size: [0.001; 3],
            })
            .collect(),
        rubble: (0..MAX_RUBBLE)
            .map(|i| RubblePose {
                id: i as u32 + 1,
                cell: 72,
                position: [0.0, 1.0, 0.0],
                rotation: Quat::IDENTITY.to_array(),
                boxes: (0..4)
                    .map(|j| {
                        SolidBox {
                            center: Vec3::X * j as f32 * 0.03,
                            half_size: Vec3::splat(0.01),
                        }
                        .into()
                    })
                    .collect(),
            })
            .collect(),
    };
    assert!(snapshot.valid());
    let bytes = crate::data::encode(&snapshot);
    eprintln!(
        "bounded cover bytes={} fragment_boxes={}",
        bytes.len(),
        MAX_RUBBLE_SOLIDS
    );
    assert!(bytes.len() <= MAX_SNAPSHOT_BYTES);
    snapshot.rubble[1].id = snapshot.rubble[0].id;
    assert!(!snapshot.valid());
    snapshot.rubble[1].id = 2;
    snapshot.rubble[0].boxes[0].half_size[0] = f32::NAN;
    assert!(!snapshot.valid());

    // Hidden cells are still reconstructed by presentation. A removal bit
    // cannot bypass the static mesh/work cap with crossing-slit descriptors.
    let cell = &cells[72];
    snapshot.rubble.clear();
    snapshot.removed = super::super::destroyed_mask(&[cell.id]);
    snapshot.cuts = (0..64)
        .map(|i| {
            let axis = i % 2;
            let mut center = Vec3::ZERO;
            center[axis] = -0.23 + (i / 2) as f32 * 0.014;
            let mut half = Vec3::from(cell.half_size);
            half[axis] = 0.001;
            Cut {
                cell: cell.id,
                center: center.into(),
                half_size: half.into(),
            }
        })
        .collect();
    assert!(
        !snapshot.valid(),
        "removed cells must obey the same reconstruction envelope"
    );
}
