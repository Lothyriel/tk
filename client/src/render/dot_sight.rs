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
}
