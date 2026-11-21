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

// Clearance comes from the narrowest opening of the current voxel optic.
// The ray is parallel to the local optical axis, never aimed at the lens center.
fn project(eye: Vec3) -> Option<Vec3> {
    let xy = eye.truncate();
    (eye.is_finite() && eye.z > 0.066 && xy.abs().max_element() <= 0.010)
        .then_some(xy.extend(-0.065))
}

fn update(
    camera: Single<(&Camera, &GlobalTransform), With<ViewModelCamera>>,
    sockets: Query<(Entity, &Name, &GlobalTransform), With<VoxelSocket>>,
    ancestry: Query<(&ChildOf, Option<&Visibility>, Has<WeaponViewModel>), Without<Dot>>,
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
        let local_eye = socket
            .affine()
            .inverse()
            .transform_point3(eye.translation());
        if let Some(lens) = project(local_eye) {
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
            let dot = project(eye).unwrap();
            assert_eq!((dot - eye).normalize(), Vec3::NEG_Z);
        }
        assert!(project(Vec3::new(0.012, 0.0, 0.21)).is_none());
        assert!(project(Vec3::new(0.0, 0.0, 0.04)).is_none());
    }
}
