//! Original stepped voxel weapon assemblies. One set of parts in weapon-local metres.
//! Base identities and authoring targets: docs/WEAPON_REFERENCES.md.
use super::{VIEW_MODEL_RENDER_LAYER, feedback};
use bevy::{camera::visibility::RenderLayers, light::NotShadowCaster, prelude::*};
use common::{WeaponKind, weapon_animation::WeaponRig};

fn boxes(parts: &[(Vec3, Vec3)]) -> Mesh {
    let mut mesh = Mesh::from(Cuboid::from_size(parts[0].0)).translated_by(parts[0].1);
    for &(size, offset) in &parts[1..] {
        mesh.merge(&Mesh::from(Cuboid::from_size(size)).translated_by(offset))
            .expect("weapon voxel attributes");
    }
    mesh
}

pub(super) fn receiver(w: WeaponKind) -> Mesh {
    let mut parts = match w {
        WeaponKind::Pistol => vec![
            (Vec3::new(0.054, 0.038, 0.23), Vec3::new(0.0, 0.015, -0.095)),
            (Vec3::new(0.052, 0.14, 0.075), Vec3::new(0.0, -0.075, 0.026)),
            (
                Vec3::new(0.052, 0.012, 0.085),
                Vec3::new(0.0, -0.064, -0.052),
            ),
            (
                Vec3::new(0.052, 0.065, 0.012),
                Vec3::new(0.0, -0.035, -0.09),
            ),
            (Vec3::new(0.035, 0.035, 0.038), Vec3::new(0.0, 0.048, 0.05)),
            (Vec3::new(0.023, 0.023, 0.19), Vec3::new(0.0, 0.055, -0.15)),
        ],
        WeaponKind::Smg => vec![
            // Open rear aperture and front blade for actual centred ADS.
            (
                Vec3::new(0.008, 0.028, 0.012),
                Vec3::new(-0.014, 0.136, 0.005),
            ),
            (
                Vec3::new(0.008, 0.028, 0.012),
                Vec3::new(0.014, 0.136, 0.005),
            ),
            (Vec3::new(0.036, 0.008, 0.012), Vec3::new(0.0, 0.151, 0.005)),
            (Vec3::new(0.008, 0.014, 0.010), Vec3::new(0.0, 0.129, -0.35)),
            (Vec3::new(0.068, 0.065, 0.30), Vec3::new(0.0, 0.045, -0.10)),
            (Vec3::new(0.054, 0.025, 0.31), Vec3::new(0.0, 0.084, -0.105)),
            (
                Vec3::new(0.050, 0.055, 0.16),
                Vec3::new(0.0, -0.005, -0.035),
            ),
            (Vec3::new(0.030, 0.030, 0.20), Vec3::new(0.0, 0.065, -0.36)),
            (Vec3::new(0.045, 0.04, 0.025), Vec3::new(0.0, 0.065, -0.448)),
            (Vec3::new(0.014, 0.045, 0.028), Vec3::new(0.0, 0.10, -0.35)),
            (Vec3::new(0.045, 0.016, 0.045), Vec3::new(0.0, 0.103, 0.005)),
        ],
        WeaponKind::Rifle => vec![
            (
                Vec3::new(0.008, 0.024, 0.012),
                Vec3::new(-0.013, 0.175, 0.005),
            ),
            (
                Vec3::new(0.008, 0.024, 0.012),
                Vec3::new(0.013, 0.175, 0.005),
            ),
            (Vec3::new(0.034, 0.008, 0.012), Vec3::new(0.0, 0.188, 0.005)),
            (Vec3::new(0.008, 0.016, 0.010), Vec3::new(0.0, 0.167, -0.43)),
            (Vec3::new(0.070, 0.075, 0.23), Vec3::new(0.0, 0.04, -0.075)),
            (Vec3::new(0.062, 0.060, 0.19), Vec3::new(0.0, -0.015, -0.06)),
            (Vec3::new(0.046, 0.028, 0.15), Vec3::new(0.0, 0.09, -0.055)),
            (Vec3::new(0.030, 0.030, 0.36), Vec3::new(0.0, 0.075, -0.51)),
            (Vec3::new(0.037, 0.037, 0.05), Vec3::new(0.0, 0.075, -0.675)),
            (Vec3::new(0.016, 0.080, 0.024), Vec3::new(0.0, 0.12, -0.43)),
            // Carry handle bridge, open below rather than a solid sight brick.
            (Vec3::new(0.035, 0.018, 0.15), Vec3::new(0.0, 0.15, -0.06)),
            (Vec3::new(0.035, 0.045, 0.02), Vec3::new(0.0, 0.125, 0.005)),
            (Vec3::new(0.035, 0.045, 0.02), Vec3::new(0.0, 0.125, -0.125)),
            (Vec3::new(0.080, 0.012, 0.022), Vec3::new(0.0, 0.083, 0.047)),
        ],
    };
    if w != WeaponKind::Pistol {
        // Open trigger guard and selector, separate from the supporting glove.
        parts.extend([
            (
                Vec3::new(0.050, 0.012, 0.075),
                Vec3::new(0.0, -0.075, -0.025),
            ),
            (
                Vec3::new(0.050, 0.055, 0.012),
                Vec3::new(0.0, -0.046, -0.06),
            ),
            (Vec3::new(0.009, 0.018, 0.04), Vec3::new(-0.039, 0.0, 0.012)),
        ]);
    }
    boxes(&parts)
}

fn furniture(w: WeaponKind) -> Mesh {
    let mut parts = match w {
        WeaponKind::Pistol => vec![
            (
                Vec3::new(0.008, 0.105, 0.061),
                Vec3::new(-0.029, -0.080, 0.03),
            ),
            (
                Vec3::new(0.008, 0.105, 0.061),
                Vec3::new(0.029, -0.080, 0.03),
            ),
        ],
        WeaponKind::Smg => vec![
            (Vec3::new(0.080, 0.066, 0.17), Vec3::new(0.0, 0.02, -0.26)),
            (Vec3::new(0.052, 0.13, 0.065), Vec3::new(0.0, -0.095, 0.035)),
            (Vec3::new(0.055, 0.055, 0.27), Vec3::new(0.0, 0.035, 0.175)),
            (Vec3::new(0.066, 0.17, 0.035), Vec3::new(0.0, -0.012, 0.31)),
        ],
        WeaponKind::Rifle => vec![
            (
                Vec3::new(0.082, 0.080, 0.245),
                Vec3::new(0.0, 0.045, -0.305),
            ),
            (Vec3::new(0.052, 0.13, 0.065), Vec3::new(0.0, -0.095, 0.035)),
            (Vec3::new(0.038, 0.038, 0.26), Vec3::new(0.0, 0.05, 0.18)),
            (Vec3::new(0.062, 0.105, 0.18), Vec3::new(0.0, 0.02, 0.23)),
            (Vec3::new(0.07, 0.15, 0.03), Vec3::new(0.0, -0.005, 0.32)),
        ],
    };
    // Stepped ribs and grip checkering are actual geometry, not flat painted details.
    for i in 0..7 {
        let f = i as f32;
        if w == WeaponKind::Pistol {
            for side in [-1.0, 1.0] {
                parts.push((
                    Vec3::new(0.010, 0.006, 0.052),
                    Vec3::new(side * 0.032, -0.12 + f * 0.013, 0.03),
                ));
            }
        } else {
            parts.push((
                Vec3::new(0.088, 0.008, 0.008),
                Vec3::new(0.0, 0.088, -0.20 - f * 0.031),
            ));
        }
    }
    boxes(&parts)
}

fn bolt(w: WeaponKind) -> Mesh {
    if w == WeaponKind::Pistol {
        let mut parts = vec![
            (
                Vec3::new(0.060, 0.050, 0.225),
                Vec3::new(0.0, 0.063, -0.105),
            ),
            (Vec3::new(0.048, 0.014, 0.21), Vec3::new(0.0, 0.095, -0.105)),
            (Vec3::new(0.009, 0.016, 0.015), Vec3::new(0.0, 0.11, -0.195)),
        ];
        for side in [-1.0, 1.0] {
            parts.push((
                Vec3::new(0.012, 0.018, 0.025),
                Vec3::new(side * 0.02, 0.11, -0.01),
            ));
            for i in 0..5 {
                parts.push((
                    Vec3::new(0.005, 0.042, 0.004),
                    Vec3::new(side * 0.032, 0.068, -0.025 - i as f32 * 0.009),
                ));
            }
        }
        boxes(&parts)
    } else if w == WeaponKind::Smg {
        boxes(&[(
            Vec3::new(0.032, 0.018, 0.038),
            Vec3::new(-0.05, 0.08, -0.22),
        )])
    } else {
        boxes(&[(
            Vec3::new(0.012, 0.027, 0.060),
            Vec3::new(0.039, 0.044, -0.045),
        )])
    }
}

fn magazine(w: WeaponKind) -> Mesh {
    let (width, height, depth) = match w {
        WeaponKind::Pistol => (0.037, 0.13, 0.052),
        WeaponKind::Smg => (0.040, 0.22, 0.055),
        WeaponKind::Rifle => (0.054, 0.19, 0.085),
    };
    let mut parts = Vec::new();
    for i in 0..5 {
        let f = i as f32 / 4.0;
        parts.push((
            Vec3::new(width, height / 5.0 + 0.001, depth),
            Vec3::new(
                0.0,
                height * (0.5 - (i as f32 + 0.5) / 5.0),
                if w == WeaponKind::Pistol {
                    0.0
                } else {
                    f * f * 0.035
                },
            ),
        ));
    }
    parts.push((
        Vec3::new(width + 0.008, 0.012, depth + 0.009),
        Vec3::new(
            0.0,
            -height * 0.5,
            if w == WeaponKind::Pistol { 0.0 } else { 0.035 },
        ),
    ));
    boxes(&parts)
}

pub(super) fn glove() -> Mesh {
    let mut parts = vec![
        (Vec3::new(0.062, 0.045, 0.07), Vec3::ZERO),
        (Vec3::new(0.058, 0.052, 0.035), Vec3::new(0.0, 0.0, 0.048)),
        (
            Vec3::new(0.022, 0.026, 0.055),
            Vec3::new(0.036, 0.014, -0.008),
        ),
    ];
    for i in 0..4 {
        let x = -0.025 + i as f32 * 0.016;
        parts.push((Vec3::new(0.014, 0.029, 0.028), Vec3::new(x, -0.014, -0.043)));
        parts.push((Vec3::new(0.014, 0.020, 0.022), Vec3::new(x, -0.034, -0.037)));
        parts.push((Vec3::new(0.013, 0.008, 0.019), Vec3::new(x, 0.027, -0.015)));
    }
    boxes(&parts)
}

fn control(w: WeaponKind) -> Mesh {
    let centre = match w {
        WeaponKind::Pistol => Vec3::new(-0.034, 0.015, -0.04),
        WeaponKind::Smg => Vec3::new(-0.04, 0.02, -0.04),
        WeaponKind::Rifle => Vec3::new(-0.047, 0.055, -0.09),
    };
    boxes(&[(Vec3::new(0.012, 0.024, 0.035), centre)])
}

pub(super) fn world_meshes(w: WeaponKind) -> [Mesh; 5] {
    [receiver(w), furniture(w), bolt(w), magazine(w), control(w)]
}

pub(super) fn steel() -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgb(0.16, 0.18, 0.20),
        metallic: 0.82,
        perceptual_roughness: 0.38,
        ..default()
    }
}

pub(super) fn trim(w: WeaponKind) -> StandardMaterial {
    StandardMaterial {
        base_color: if w == WeaponKind::Pistol {
            Color::srgb(0.20, 0.085, 0.038)
        } else {
            Color::srgb(0.045, 0.055, 0.055)
        },
        perceptual_roughness: 0.78,
        ..default()
    }
}

fn part(
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
) -> impl Bundle {
    (
        Mesh3d(mesh),
        MeshMaterial3d(material),
        transform,
        RenderLayers::layer(VIEW_MODEL_RENDER_LAYER),
        NotShadowCaster,
    )
}

fn framed_part(
    meshes: &mut Assets<Mesh>,
    mesh: Mesh,
    material: Handle<StandardMaterial>,
    transform: Transform,
) -> impl Bundle {
    let bounds = view_frame_bounds(&mesh);
    (bounds, part(meshes.add(mesh), material, transform))
}

fn view_frame_bounds(mesh: &Mesh) -> feedback::ViewFrameBounds {
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        unreachable!("voxel parts have position vertices")
    };
    let min = vertices
        .iter()
        .map(|p| Vec3::from(*p))
        .fold(Vec3::splat(f32::INFINITY), Vec3::min);
    let max = vertices
        .iter()
        .map(|p| Vec3::from(*p))
        .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
    let bounds = std::array::from_fn(|i| {
        Vec3::new(
            if i & 1 == 0 { min.x } else { max.x },
            if i & 2 == 0 { min.y } else { max.y },
            if i & 4 == 0 { min.z } else { max.z },
        )
    });
    feedback::ViewFrameBounds(bounds)
}

pub(super) fn assembly(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    w: WeaponKind,
    offset: Vec3,
) -> impl Bundle {
    let rig = WeaponRig::for_weapon(w);
    let metal = materials.add(steel());
    let trim = materials.add(trim(w));
    let leather = materials.add(Color::srgb(0.11, 0.14, 0.10));
    let sleeve = materials.add(Color::srgb(0.12, 0.31, 0.32));
    let hand_mesh = glove();
    let hand_bounds = view_frame_bounds(&hand_mesh);
    let hand = meshes.add(hand_mesh);
    let flash = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.75, 0.1),
        emissive: LinearRgba::rgb(14.0, 6.0, 0.5),
        unlit: true,
        ..default()
    });
    (
        Transform::from_translation(offset),
        Visibility::Inherited,
        children![
            framed_part(meshes, receiver(w), metal.clone(), Transform::default()),
            framed_part(meshes, furniture(w), trim, Transform::default()),
            (
                feedback::Bolt,
                framed_part(meshes, bolt(w), metal.clone(), Transform::default())
            ),
            (
                feedback::Control,
                framed_part(meshes, control(w), metal.clone(), Transform::default())
            ),
            (
                feedback::Magazine,
                framed_part(
                    meshes,
                    magazine(w),
                    metal,
                    Transform::from_translation(rig.magazine)
                )
            ),
            (
                hand_bounds,
                part(
                    hand.clone(),
                    leather.clone(),
                    Transform::from_translation(rig.trigger)
                )
            ),
            (
                feedback::SupportHand,
                hand_bounds,
                part(hand, leather, Transform::from_translation(rig.support))
            ),
            (
                feedback::Forearm { support: true },
                part(
                    meshes.add(Cuboid::new(0.07, 0.28, 0.065)),
                    sleeve.clone(),
                    Transform::default()
                )
            ),
            (
                feedback::Forearm { support: false },
                part(
                    meshes.add(Cuboid::new(0.075, 0.28, 0.07)),
                    sleeve.clone(),
                    Transform::default()
                )
            ),
            (
                feedback::UpperArm { support: true },
                part(
                    meshes.add(Cuboid::new(0.08, 0.28, 0.075)),
                    sleeve.clone(),
                    Transform::default()
                )
            ),
            (
                feedback::UpperArm { support: false },
                part(
                    meshes.add(Cuboid::new(0.085, 0.28, 0.08)),
                    sleeve,
                    Transform::default()
                )
            ),
            (
                feedback::Flash,
                Visibility::Hidden,
                part(
                    meshes.add(Cuboid::new(0.075, 0.075, 0.12)),
                    flash,
                    Transform::from_translation(rig.muzzle - Vec3::Z * 0.05)
                )
            ),
        ],
    )
}

pub(super) fn view_offset(w: WeaponKind) -> Vec3 {
    match w {
        WeaponKind::Pistol => Vec3::new(0.13, -0.17, -0.60),
        WeaponKind::Smg => Vec3::new(0.17, -0.18, -0.62),
        WeaponKind::Rifle => Vec3::new(0.17, -0.19, -0.66),
    }
}

pub(super) fn framed_offset(w: WeaponKind, aspect: f32) -> Vec3 {
    let mut offset = view_offset(w);
    offset.x *= aspect.clamp(0.4, 1.0);
    offset
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn primary_ads_puts_the_butt_end_behind_the_eye_not_in_front_of_the_iron_sights() {
        for (weapon, butt_z) in [(WeaponKind::Smg, 0.31), (WeaponKind::Rifle, 0.32)] {
            let eye_relative = common::weapon_animation::ads_offset(weapon) + Vec3::Z * butt_z;
            assert!(
                eye_relative.z > 0.0,
                "{weapon:?}: butt plate should lie behind the ADS eye"
            );
        }
    }
    #[test]
    fn zoom_extremes_and_portrait_to_ultrawide_keep_weapon_contacts_in_frame() {
        for w in [WeaponKind::Pistol, WeaponKind::Smg, WeaponKind::Rifle] {
            let rig = WeaponRig::for_weapon(w);
            for world_fov in [20.0_f32, 90.0, 160.0] {
                let vertical = (world_fov.clamp(65.0, 100.0).to_radians() * 0.5).tan();
                for aspect in [9.0 / 16.0, 4.0 / 3.0, 16.0 / 10.0, 16.0 / 9.0, 21.0 / 9.0] {
                    for anchor in [rig.trigger, rig.support, rig.muzzle] {
                        let p = framed_offset(w, aspect) + anchor;
                        assert!(p.z < 0.0);
                        assert!(p.y.abs() / -p.z < vertical);
                        assert!(
                            p.x.abs() / -p.z < vertical * aspect,
                            "{w:?} {world_fov}/{aspect}: {p:?}"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn both_perspectives_use_geometry_that_ends_at_the_named_muzzle() {
        use bevy::mesh::VertexAttributeValues;
        for w in [WeaponKind::Pistol, WeaponKind::Smg, WeaponKind::Rifle] {
            let parts = world_meshes(w);
            let mut front = f32::INFINITY;
            for mesh in &parts[..3] {
                let Some(VertexAttributeValues::Float32x3(vertices)) =
                    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!("real weapon positions required");
                };
                for v in vertices {
                    assert!(Vec3::from(*v).is_finite());
                    front = front.min(v[2]);
                }
            }
            assert!(
                (front - WeaponRig::for_weapon(w).muzzle.z).abs() < 0.003,
                "{w:?} geometry={front}"
            );
        }
    }
    #[test]
    fn weapons_are_detailed_original_assemblies_and_hands_are_framed() {
        for w in [WeaponKind::Pistol, WeaponKind::Smg, WeaponKind::Rifle] {
            assert!(receiver(w).count_vertices() >= 144);
            assert!(furniture(w).count_vertices() >= 240);
            assert!(glove().count_vertices() >= 300);
            let rig = WeaponRig::for_weapon(w);
            for p in [rig.support, rig.trigger, rig.muzzle] {
                let p = p + view_offset(w);
                assert!(p.z < -0.4);
                assert!(p.y.abs() / -p.z < (65.0_f32.to_radians() * 0.5).tan());
                assert!(p.x.abs() / -p.z < (65.0_f32.to_radians() * 0.5).tan() * 4.0 / 3.0);
            }
        }
    }
}
