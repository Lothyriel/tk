//! Two authored glove poses share the rest topology. GPU morph weights follow
//! the already filtered contact weights; no per-frame mesh allocation.
use super::*;
use bevy::mesh::morph::{MeshMorphWeights, MorphAttributes};

pub(crate) fn weights(action: &common::weapon_animation::WeaponAction) -> [f32; 2] {
    [
        action.magazine_grip * (1.0 - action.control_grip),
        action.control_grip,
    ]
}

pub(crate) fn component(weights: [f32; 2]) -> MeshMorphWeights {
    MeshMorphWeights::Value {
        weights: weights.to_vec(),
    }
}

pub(crate) fn bounds() -> bevy::camera::primitives::Aabb {
    // Covers all three poses and the anatomical fallback hand. GPU morphs do
    // not update CPU culling bounds when the thumb/fingers move beyond rest.
    bevy::camera::primitives::Aabb::from_min_max(Vec3::splat(-0.10), Vec3::splat(0.10))
}

/// Same linear morph composition used by the GPU, for camera bounds and
/// geometric acceptance against the actual mesh rather than a proxy palm.
pub(crate) fn position(mesh: &Mesh, index: usize, base: Vec3, weights: &[f32]) -> Vec3 {
    let Some(targets) = mesh.morph_targets() else {
        return base;
    };
    weights
        .iter()
        .enumerate()
        .fold(base, |point, (target, weight)| {
            point + targets[target * mesh.count_vertices() + index].position * *weight
        })
}

pub(crate) fn mesh() -> Mesh {
    let mut rest = glove();
    let mut magazine = vec![
        (
            Vec3::new(0.030, 0.055, 0.055),
            Vec3::new(-0.018, 0.0, 0.004),
        ),
        (
            Vec3::new(0.030, 0.055, 0.030),
            Vec3::new(-0.018, 0.0, 0.046),
        ),
        (
            Vec3::new(0.028, 0.014, 0.012),
            Vec3::new(0.009, 0.030, 0.034),
        ),
    ];
    let mut control = vec![
        (
            Vec3::new(0.030, 0.050, 0.050),
            Vec3::new(-0.047, -0.010, 0.020),
        ),
        (
            Vec3::new(0.030, 0.050, 0.030),
            Vec3::new(-0.060, -0.020, 0.060),
        ),
        // Thumb pad reaches the authored control contact; the palm and curled
        // fingers stay outside the receiver's left wall.
        (Vec3::new(0.030, 0.014, 0.014), Vec3::new(-0.015, 0.0, 0.0)),
    ];
    for i in 0..4 {
        let y = -0.027 + i as f32 * 0.017;
        magazine.extend([
            (Vec3::new(0.012, 0.014, 0.040), Vec3::new(-0.006, y, -0.030)),
            (Vec3::new(0.040, 0.014, 0.012), Vec3::new(0.014, y, -0.054)),
            (Vec3::new(0.012, 0.014, 0.028), Vec3::new(0.035, y, -0.036)),
        ]);
        control.extend([
            (Vec3::new(0.014, 0.014, 0.028), Vec3::new(-0.049, y, -0.017)),
            (Vec3::new(0.020, 0.014, 0.012), Vec3::new(-0.038, y, -0.026)),
            (Vec3::new(0.014, 0.014, 0.020), Vec3::new(-0.027, y, -0.012)),
        ]);
    }
    let positions = |mesh: &Mesh, attribute| {
        let bevy::mesh::VertexAttributeValues::Float32x3(values) =
            mesh.attribute(attribute).unwrap()
        else {
            unreachable!("glove cuboids")
        };
        values.iter().copied().map(Vec3::from).collect::<Vec<_>>()
    };
    let base_positions = positions(&rest, Mesh::ATTRIBUTE_POSITION);
    let base_normals = positions(&rest, Mesh::ATTRIBUTE_NORMAL);
    let targets = [boxes(&magazine), boxes(&control)]
        .into_iter()
        .flat_map(|target| {
            positions(&target, Mesh::ATTRIBUTE_POSITION)
                .into_iter()
                .zip(positions(&target, Mesh::ATTRIBUTE_NORMAL))
                .zip(base_positions.iter().zip(&base_normals))
                .map(|((position, normal), (base, base_normal))| {
                    MorphAttributes::new(position - base, normal - base_normal, Vec3::ZERO)
                })
                .collect::<Vec<_>>()
        })
        .collect();
    rest.set_morph_targets(targets);
    rest
}

#[cfg(test)]
pub(crate) fn assert_magwell_clear(world: &World, palm: Entity, lower: Entity, fraction: f32) {
    use bevy::mesh::VertexAttributeValues;
    static WALLS: std::sync::OnceLock<Vec<(Vec3, Vec3)>> = std::sync::OnceLock::new();
    let walls = WALLS.get_or_init(|| {
        let source: serde_json::Value =
            serde_json::from_str(include_str!("../../../../assets/voxel/m4a1_refined.json"))
                .unwrap();
        let lower = source["parts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "lower")
            .unwrap();
        lower["boxes"].as_array().unwrap()[1..5]
            .iter()
            .map(|wall| {
                let vector = |name: &str| {
                    Vec3::from_array(std::array::from_fn(|i| {
                        wall[name][i].as_f64().unwrap() as f32
                    }))
                };
                // Contact tolerance is 1 mm; do not fill the open magazine channel.
                (
                    vector("min") + Vec3::splat(0.001),
                    vector("max") - Vec3::splat(0.001),
                )
            })
            .collect()
    });
    let to_lower = world
        .get::<GlobalTransform>(lower)
        .unwrap()
        .to_matrix()
        .inverse()
        * world.get::<GlobalTransform>(palm).unwrap().to_matrix();
    let mesh = world
        .resource::<Assets<Mesh>>()
        .get(&world.get::<Mesh3d>(palm).unwrap().0)
        .unwrap();
    let VertexAttributeValues::Float32x3(vertices) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
    else {
        panic!("positions");
    };
    let MeshMorphWeights::Value { weights } = world.get::<MeshMorphWeights>(palm).unwrap() else {
        panic!("per-hand weights");
    };
    let points: Vec<_> = vertices
        .iter()
        .enumerate()
        .map(|(i, p)| to_lower.transform_point3(position(mesh, i, Vec3::from(*p), weights)))
        .collect();
    let indices: Vec<_> = mesh.indices().unwrap().iter().collect();
    for (min, max) in walls {
        let centre = (min + max) * 0.5;
        let half = (max - min) * 0.5;
        for triangle in indices.as_chunks::<3>().0 {
            let p: [Vec3; 3] = std::array::from_fn(|i| points[triangle[i]] - centre);
            assert!(
                !triangle_intersects_box(p, half),
                "support glove triangle penetrates magwell at {fraction}, weights={weights:?}, points={:?}",
                p.map(|point| point + centre)
            );
        }
    }
}

#[cfg(test)]
pub(crate) fn triangle_intersects_box(p: [Vec3; 3], half: Vec3) -> bool {
    let edges = [p[1] - p[0], p[2] - p[1], p[0] - p[2]];
    // Box is centred at zero. Include edge axes: a finger face can cross a
    // thin wall without any of its vertices being inside that wall.
    ![Vec3::X, Vec3::Y, Vec3::Z, edges[0].cross(edges[1])]
        .into_iter()
        .chain(
            edges
                .into_iter()
                .flat_map(|edge| [Vec3::X, Vec3::Y, Vec3::Z].map(|axis| edge.cross(axis))),
        )
        .any(|axis| {
            let projections = p.map(|point| point.dot(axis));
            let radius = half.dot(axis.abs());
            projections.into_iter().fold(f32::INFINITY, f32::min) > radius
                || projections.into_iter().fold(f32::NEG_INFINITY, f32::max) < -radius
        })
}
