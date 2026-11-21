//! First-person game reticle projected through the authored optic tube.
use super::{VIEW_MODEL_RENDER_LAYER, ViewModelCamera, WeaponViewModel, voxel::VoxelSocket};
use bevy::{
    camera::visibility::{RenderLayers, VisibilitySystems},
    light::NotShadowCaster,
    prelude::*,
    transform::TransformSystems,
};

pub(super) struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn).add_systems(
            PostUpdate,
            update
                .after(TransformSystems::Propagate)
                .before(VisibilitySystems::VisibilityPropagate),
        );
    }
}

#[derive(Component)]
struct Dot;

fn spawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Dot,
        Mesh3d(meshes.add(Sphere::new(1.0).mesh().uv(8, 6))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.08, 0.035),
            unlit: true,
            ..default()
        })),
        RenderLayers::layer(VIEW_MODEL_RENDER_LAYER),
        NotShadowCaster,
        Visibility::Hidden,
    ));
}

// The ray is parallel to the local optical axis, never aimed at the lens center.
fn project(eye: Vec3, min: Vec3, max: Vec3) -> Option<Vec3> {
    let xy = eye.truncate();
    (eye.is_finite()
        && min.cmplt(max).all()
        && eye.z > max.z
        && xy.cmpge(min.truncate()).all()
        && xy.cmple(max.truncate()).all())
    .then_some(xy.extend(min.z))
}

fn update(
    camera: Single<(&Camera, &GlobalTransform), With<ViewModelCamera>>,
    sockets: Query<(Entity, &Name, &GlobalTransform), With<VoxelSocket>>,
    ancestry: Query<(&ChildOf, Option<&Visibility>, Has<WeaponViewModel>), Without<Dot>>,
    children: Query<&Children>,
    mut dot: Single<
        (&mut GlobalTransform, &mut Visibility),
        (With<Dot>, Without<ViewModelCamera>, Without<VoxelSocket>),
    >,
) {
    let (camera, eye) = *camera;
    let (transform, visibility) = &mut *dot;
    **visibility = Visibility::Hidden;
    if !camera.is_active {
        return;
    }
    for (entity, name, socket) in &sockets {
        if name.as_str() != "sight.axis" {
            continue;
        }
        let mut cursor = entity;
        let mut equipped = false;
        while let Ok((parent, visibility, view)) = ancestry.get(cursor) {
            if visibility == Some(&Visibility::Hidden) {
                break;
            }
            if view {
                equipped = true;
                break;
            }
            cursor = parent.0;
        }
        if !equipped {
            continue;
        }
        let Ok((parent, _, _)) = ancestry.get(entity) else {
            continue;
        };
        let Ok(siblings) = children.get(parent.0) else {
            continue;
        };
        let to_axis = socket.affine().inverse();
        let corner = |label| {
            siblings.iter().find_map(|child| {
                let (_, name, pose) = sockets.get(child).ok()?;
                (name.as_str() == label).then(|| to_axis.transform_point3(pose.translation()))
            })
        };
        let (Some(min), Some(max)) = (corner("sight.aperture_min"), corner("sight.aperture_max"))
        else {
            continue;
        };
        let local_eye = to_axis.transform_point3(eye.translation());
        if let Some(lens) = project(local_eye, min, max) {
            let point = socket.transform_point(lens);
            // Scale the mesh with eye distance to keep a small apparent dot.
            let radius = point.distance(eye.translation()) * 0.0023;
            **transform = GlobalTransform::from(
                Transform::from_translation(point).with_scale(Vec3::splat(radius)),
            );
            **visibility = Visibility::Visible;
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Vec3 = Vec3::new(-0.008, -0.006, -0.065);
    const MAX: Vec3 = Vec3::new(0.009, 0.007, 0.066);

    #[test]
    fn dot_sight_system_uses_visible_first_person_socket_only() {
        let mut app = App::new();
        app.add_systems(Update, update);
        app.world_mut().spawn((
            ViewModelCamera,
            Camera::default(),
            GlobalTransform::from(Transform::from_xyz(0.007, 0.0, 0.21)),
        ));
        let root = app.world_mut().spawn_empty().id();
        let view = app
            .world_mut()
            .spawn((
                WeaponViewModel {
                    weapon: common::WeaponKind::Rifle,
                },
                ChildOf(root),
                Visibility::Inherited,
            ))
            .id();
        app.world_mut().spawn((
            ChildOf(view),
            VoxelSocket,
            Name::new("sight.axis"),
            GlobalTransform::IDENTITY,
        ));
        for (name, point) in [("sight.aperture_min", MIN), ("sight.aperture_max", MAX)] {
            app.world_mut().spawn((
                ChildOf(view),
                VoxelSocket,
                Name::new(name),
                GlobalTransform::from_translation(point),
            ));
        }
        let dot = app
            .world_mut()
            .spawn((Dot, GlobalTransform::IDENTITY, Visibility::Hidden))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(dot),
            Some(&Visibility::Visible)
        );
        assert_eq!(
            app.world()
                .get::<GlobalTransform>(dot)
                .unwrap()
                .translation(),
            Vec3::new(0.007, 0.0, -0.065)
        );
        app.world_mut().entity_mut(view).insert(Visibility::Hidden);
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(dot),
            Some(&Visibility::Hidden)
        );
    }

    #[test]
    fn dot_sight_is_parallel_inside_aperture_and_hidden_outside() {
        for eye in [Vec3::new(0.0, 0.0, 0.21), Vec3::new(0.007, -0.004, 0.25)] {
            let dot = project(eye, MIN, MAX).unwrap();
            assert_eq!((dot - eye).normalize(), Vec3::NEG_Z);
        }
        assert!(project(Vec3::new(0.0095, 0.0, 0.21), MIN, MAX).is_none());
        assert!(project(Vec3::new(0.0, 0.0, 0.04), MIN, MAX).is_none());
    }

    #[test]
    fn dot_sight_projects_authored_optic_at_different_viewport_aspects() {
        use bevy::camera::CameraProjection;
        use common::catalog::{AnchorKind, Attachment, DefinitionId, authored_rig, catalog};

        let catalog = catalog();
        let mut config = catalog.configuration(common::WeaponId::M4A1).unwrap();
        let mut compact = config.clone();
        for part in &mut compact.components {
            if let Some(definition) = match part.node {
                1 => Some(120),
                2 => Some(122),
                3 => Some(124),
                4 => Some(126),
                _ => None,
            } {
                part.definition = DefinitionId(definition);
            }
        }
        let irons = catalog.resolve(&compact).unwrap().assembly;
        config.components.extend([
            Attachment {
                node: 9,
                definition: DefinitionId(132),
                parent: None,
                socket: "top".into(),
                position: 3,
            },
            Attachment {
                node: 10,
                definition: DefinitionId(133),
                parent: Some(9),
                socket: "optic".into(),
                position: 0,
            },
        ]);
        let spec = catalog.resolve(&config).unwrap();
        let part = spec
            .assembly
            .parts
            .iter()
            .find(|part| part.node == 10)
            .unwrap();
        let rig = authored_rig("m4a1_pro").unwrap();
        let optic = spec.assembly.ads_from_root * part.pose;
        let axis = optic * rig.sockets["sight.axis"].transform();
        let to_axis = axis.to_matrix().inverse();
        let corner = |label: &str| {
            to_axis.transform_point3((optic * rig.sockets[label].transform()).translation)
        };
        let min = corner("sight.aperture_min");
        let max = corner("sight.aperture_max");
        let to_view = axis.to_matrix();
        let centered_eye = to_axis.transform_point3(Vec3::ZERO);
        let mut app = App::new();
        let camera = app
            .world_mut()
            .spawn(super::super::view_model_camera())
            .id();
        let Projection::Perspective(base) = app.world().get::<Projection>(camera).unwrap() else {
            panic!("expected the production perspective lens");
        };
        assert!((base.fov.to_degrees() - 65.0).abs() < 1e-4);
        assert_eq!(base.near, 0.1);
        for (width, height) in [(1200.0, 600.0), (900.0, 700.0), (600.0, 900.0)] {
            let mut lens = base.clone();
            lens.update(width, height);
            let clip = lens.get_clip_from_view();
            let ndc = |p: Vec3| {
                let p = clip * p.extend(1.0);
                p.truncate() / p.w
            };
            for anchor in [AnchorKind::RearIron, AnchorKind::FrontIron] {
                let sight = ndc(irons
                    .ads_from_root
                    .transform_point(irons.anchors[&anchor].translation));
                assert!(sight.truncate().length() < 0.004, "iron off centre");
                assert!((0.0..=1.0).contains(&sight.z), "iron clipped");
            }
            for roll in [-0.3, 0.3] {
                // Roll both the camera and the rig, then add a relative angular
                // offset to make a hardcoded centre dot fail the comparison.
                let roll = Quat::from_rotation_z(roll);
                let aim = roll * Quat::from_rotation_y(0.055);
                let direction = roll.inverse() * (aim * (axis.rotation * Vec3::NEG_Z));
                let expected = ndc(direction).truncate();
                assert!(expected.length() > 0.02);
                for shift in [0.0, 0.007, -0.007] {
                    let eye = centered_eye + Vec3::new(shift, -0.5 * shift, 0.0);
                    let point = project(eye, min, max).unwrap();
                    let view =
                        |local| roll.inverse() * (aim * to_view.transform_vector3(local - eye));
                    let projected = ndc(view(point));
                    assert!(projected.truncate().distance(expected) < 1e-5);
                    assert!((0.0..=1.0).contains(&projected.z), "dot clipped");
                    // A decal glued to the lens would slide on screen as the eye moves.
                    if shift != 0.0 {
                        let decal = ndc(view(Vec3::new(0.0, 0.0, min.z)));
                        assert!(projected.truncate().distance(decal.truncate()) > 0.01);
                    }
                }
                assert!(project(centered_eye.with_x(max.x + 0.001), min, max).is_none());
                assert!(project(centered_eye.with_x(min.x - 0.001), min, max).is_none());
            }
        }
    }
}
