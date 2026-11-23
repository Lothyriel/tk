//! Tunable game-material response. Fine impact detail stays local; no dense
//! millimetre grid or whole-cell health threshold is involved.
use super::*;

#[derive(Clone, Copy)]
pub struct Material {
    pub density: f32,
    pub compression: f32,
    pub shear: f32,
    pub degradation: f32,
    pub cluster_distance: f32,
    /// Removed dust/flakes below these game presentation thresholds are
    /// accounted fines, independently of the remaining rigid-body budget.
    pub chip_mass_kg: f32,
    pub chip_thickness: f32,
}

pub fn properties(surface: SurfaceKind) -> Material {
    match surface {
        SurfaceKind::Wood => Material {
            density: 500.0,
            compression: 180_000.0,
            shear: 45_000.0,
            degradation: 70.0,
            cluster_distance: 0.14,
            chip_mass_kg: 0.001,
            chip_thickness: 0.0005,
        },
        SurfaceKind::Concrete => Material {
            density: 1800.0,
            compression: 900_000.0,
            shear: 120_000.0,
            degradation: 120.0,
            cluster_distance: 0.09,
            chip_mass_kg: 0.002,
            chip_thickness: 0.001,
        },
    }
}

pub fn physical_debris(surface: SurfaceKind, solid: SolidBox) -> bool {
    let material = properties(surface);
    let mass = solid.volume() * material.density;
    // A substantial thin sheet remains physical; only small flakes and dust
    // are fines. These thresholds never depend on queue/body/shape pressure.
    mass >= material.chip_mass_kg
        && (solid.half_size.min_element() * 2.0 >= material.chip_thickness
            || mass >= material.chip_mass_kg * 4.0)
}

#[derive(Clone, Copy)]
pub struct Impact {
    pub point: Vec3,
    pub normal: Vec3,
    /// Original exterior mouth of this local damage cluster. Later impacts
    /// may hit the recessed backstop or a cavity side rather than this plane.
    pub entry: Vec3,
    pub entry_normal: Vec3,
    pub direction: Vec3,
    pub diameter: f32,
    pub damage: f32,
    pub penetration: f32,
    pub dose: f32,
    pub seed: u32,
}

/// World-space cutters, independent of the old structural-cell seams. A bore
/// follows the ray in bounded steps; degraded rims have real irregular terraces.
pub fn cutters(bounds: SolidBox, surface: SurfaceKind, impact: Impact) -> Vec<SolidBox> {
    let Impact {
        point,
        normal,
        entry,
        entry_normal,
        direction,
        diameter,
        damage,
        penetration,
        dose,
        seed,
    } = impact;
    if damage <= 0.0 || penetration <= 0.0 || !direction.is_finite() {
        return vec![];
    }
    let axis = (0..3)
        .max_by(|&a, &b| normal[a].abs().total_cmp(&normal[b].abs()))
        .unwrap();
    let tangent: Vec<_> = (0..3).filter(|&a| a != axis).collect();
    let direction = direction.normalize_or_zero();
    let incidence = direction.dot(-normal).max(0.05);
    let material = properties(surface);
    let excess = (dose - material.degradation).max(0.0);
    let radius = (diameter * 0.5 + excess.sqrt() * 0.005).min(0.19);
    let mut exit = f32::INFINITY;
    for i in 0..3 {
        if direction[i].abs() > 1e-6 {
            let edge = bounds.center[i] + bounds.half_size[i] * direction[i].signum();
            exit = exit.min(((edge - point[i]) / direction[i]).max(0.0));
        }
    }
    let depth = match surface {
        SurfaceKind::Wood => exit.min(penetration / 20.0),
        SurfaceKind::Concrete => (0.015 * (damage / 34.0).clamp(0.01, 3.0) * incidence).min(exit),
    };
    if !depth.is_finite() || depth < 1e-6 {
        return vec![];
    }
    let transverse = direction[tangent[0]].abs().max(direction[tangent[1]].abs());
    let steps = ((depth * transverse / (diameter * 0.5)).ceil() as usize).clamp(1, 64);
    let mut result = Vec::new();
    for step in 0..steps {
        let a = point + direction * (depth * step as f32 / steps as f32);
        let b = point + direction * (depth * (step + 1) as f32 / steps as f32);
        let mut half = (b - a).abs() * 0.5 + Vec3::splat(diameter * 0.5);
        // Longitudinal overlap only prevents numeric cracks. Adding the bore
        // radius along this axis as well widens a diagonal into a fat gash.
        half[axis] = (b[axis] - a[axis]).abs() * 0.5 + 0.00002;
        // A concrete chip's mouth is wider than its advancing material depth.
        if matches!(surface, SurfaceKind::Concrete) {
            half[tangent[0]] += diameter;
            half[tangent[1]] += diameter;
        }
        result.push(SolidBox {
            center: (a + b) * 0.5,
            half_size: half,
        });
    }
    if excess > 0.0 {
        let rim_axis = (0..3)
            .max_by(|&a, &b| entry_normal[a].abs().total_cmp(&entry_normal[b].abs()))
            .unwrap();
        let rim_tangent: Vec<_> = (0..3).filter(|&a| a != rim_axis).collect();
        let inner = point + direction * depth;
        let stretch = if matches!(surface, SurfaceKind::Wood) {
            [0.65, 1.6]
        } else {
            [1.0, 1.0]
        };
        for row in 0..5 {
            let v = (row as f32 + 0.5) / 2.5 - 1.0;
            let noise = 0.82
                + ((seed.wrapping_mul(1664525).wrapping_add(row * 1013904223)) % 19) as f32 * 0.01;
            let mut center = point + direction * (depth * 0.5);
            center[rim_axis] = (entry[rim_axis] + inner[rim_axis]) * 0.5;
            center[rim_tangent[1]] += v * radius * stretch[1];
            let mut half = Vec3::ZERO;
            // Grow the LOCAL rough mouth back to its recorded entry plane.
            // A wide pocket beginning only at the latest recessed hit traps
            // its own larger chips behind the previous narrow front opening.
            half[rim_axis] = (entry[rim_axis] - inner[rim_axis]).abs() * 0.5 + 0.0002;
            half[rim_tangent[0]] = radius * stretch[0] * (1.0 - v * v).sqrt() * noise;
            half[rim_tangent[1]] = radius * stretch[1] / 5.0;
            result.push(SolidBox {
                center,
                half_size: half,
            });
        }
    }
    result
}

/// Split surviving material at nonuniform planes before fragment grouping.
/// All child volumes sum exactly to their source, including old holes/chips.
pub fn fracture_boxes(solid: SolidBox, seed: u32) -> Vec<SolidBox> {
    let mut result = vec![solid];
    for axis in [0, 1] {
        result = result
            .into_iter()
            .flat_map(|part| {
                if part.half_size[axis] < 0.09 {
                    return vec![part];
                }
                let fraction = 0.32
                    + (seed.wrapping_mul(2654435761).rotate_left(axis as u32 * 7) % 37) as f32
                        * 0.01;
                let length = part.half_size[axis] * 2.0;
                let lo = part.center[axis] - part.half_size[axis];
                let mut left = part;
                let mut right = part;
                left.half_size[axis] = length * fraction * 0.5;
                left.center[axis] = lo + left.half_size[axis];
                right.half_size[axis] = length * (1.0 - fraction) * 0.5;
                right.center[axis] = lo + length - right.half_size[axis];
                vec![left, right]
            })
            .collect();
    }
    result
}
