use bevy::{
    camera::visibility::RenderLayers, color::palettes::tailwind, light::NotShadowCaster,
    prelude::*,
};
use common::{
    Client, ClientInput, Lobby, PlayerId, PlayerVisualState, PLAYER_CROUCH_SCALE,
    PLAYER_CROUCH_VIEW_OFFSET,
};

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        let startup_systems = (spawn_view_model, spawn_world_model, spawn_lights);

        app.add_systems(Startup, startup_systems)
            .add_systems(Update, (change_fov, sync_local_view, sync_player_visuals));
    }
}

#[derive(Debug, Component)]
struct WorldModelCamera;

#[derive(Debug, Component)]
struct LocalView;

#[derive(Debug, Component)]
pub struct PlayerBodyVisual;

#[derive(Debug, Component)]
pub struct ProjectileVisual {
    pub id: u64,
}

#[derive(Debug, Component)]
pub struct ImpactMarkVisual {
    pub id: u64,
}

/// Used implicitly by all entities without a `RenderLayers` component.
/// Our world model camera and all objects other than the player are on this layer.
/// The light source belongs to both layers.
const DEFAULT_RENDER_LAYER: usize = 0;

/// Used by the view model camera and the player's arm.
/// The light source belongs to both layers.
const VIEW_MODEL_RENDER_LAYER: usize = 1;

fn spawn_view_model(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lobby: ResMut<Lobby>,
    player_id: Res<PlayerId>,
) {
    // save meshes and materials handles so we can reutilize
    let arm = meshes.add(Cuboid::new(0.1, 0.1, 0.5));
    let arm_material = materials.add(Color::from(tailwind::TEAL_200));

    let player = commands
        .spawn((
            Client { id: player_id.0 },
            PlayerId(player_id.0),
            Transform::from_xyz(0.0, 1.0, 0.0),
            Visibility::default(),
            children![
                world_camera(),
                // Spawn view model camera.
                view_model_camera(),
                // Spawn the player's right arm.
                player_right_arm(arm, arm_material),
            ],
            PlayerVisualState::default(),
        ))
        .id();

    lobby.players.insert(player_id.0, player);
}

fn world_camera() -> impl Bundle {
    (
        LocalView,
        WorldModelCamera,
        Camera3d::default(),
        Transform::default(),
        Projection::from(PerspectiveProjection {
            fov: 90.0_f32.to_radians(),
            ..default()
        }),
    )
}

fn view_model_camera() -> impl Bundle {
    (
        LocalView,
        Camera3d::default(),
        Camera {
            // Bump the order to render on top of the world model.
            order: 1,
            ..default()
        },
        Transform::default(),
        Projection::from(PerspectiveProjection {
            fov: 70.0_f32.to_radians(),
            ..default()
        }),
        // Only render objects belonging to the view model.
        RenderLayers::layer(VIEW_MODEL_RENDER_LAYER),
    )
}

fn player_right_arm(arm: Handle<Mesh>, arm_material: Handle<StandardMaterial>) -> impl Bundle {
    (
        LocalView,
        Mesh3d(arm),
        MeshMaterial3d(arm_material),
        Transform::from_xyz(0.2, -0.1, -0.25),
        // Ensure the arm is only rendered by the view model camera.
        RenderLayers::layer(VIEW_MODEL_RENDER_LAYER),
        // The arm is free-floating, so shadows would look weird.
        NotShadowCaster,
    )
}

fn spawn_world_model(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let floor = meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(10.0)));
    let cube = meshes.add(Cuboid::new(2.0, 0.5, 1.0));
    let material = materials.add(Color::WHITE);

    // The world model camera will render the floor and the cubes spawned in this system.
    // Assigning no `RenderLayers` component defaults to layer 0.

    commands.spawn((Mesh3d(floor), MeshMaterial3d(material.clone())));

    commands.spawn((
        Mesh3d(cube.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_xyz(0.0, 0.25, -3.0),
    ));

    commands.spawn((
        Mesh3d(cube),
        MeshMaterial3d(material),
        Transform::from_xyz(0.75, 1.75, 0.0),
    ));
}

fn spawn_lights(mut commands: Commands) {
    commands.spawn((
        PointLight {
            color: Color::from(tailwind::ROSE_300),
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(-2.0, 4.0, -0.75),
        // The light source illuminates both the world model and the view model.
        RenderLayers::from_layers(&[DEFAULT_RENDER_LAYER, VIEW_MODEL_RENDER_LAYER]),
    ));
}

fn change_fov(
    input: Res<ButtonInput<KeyCode>>,
    mut world_model_projection: Single<&mut Projection, With<WorldModelCamera>>,
) {
    let Projection::Perspective(perspective) = world_model_projection.as_mut() else {
        unreachable!(
            "The `Projection` component was explicitly built with `Projection::Perspective`"
        );
    };

    if input.pressed(KeyCode::ArrowUp) {
        perspective.fov -= 1.0_f32.to_radians();
        perspective.fov = perspective.fov.max(20.0_f32.to_radians());
    }
    if input.pressed(KeyCode::ArrowDown) {
        perspective.fov += 1.0_f32.to_radians();
        perspective.fov = perspective.fov.min(160.0_f32.to_radians());
    }
}

fn sync_local_view(
    player_input: Res<ClientInput>,
    player_visual_state: Single<&PlayerVisualState, With<PlayerId>>,
    mut query: Query<&mut Transform, With<LocalView>>,
) {
    let vertical_offset = if player_visual_state.crouched {
        PLAYER_CROUCH_VIEW_OFFSET
    } else {
        0.0
    };
    let rotation = Quat::from_euler(
        EulerRot::YXZ,
        0.0,
        player_input.camera.pitch,
        player_input.camera.roll,
    );

    for mut transform in query.iter_mut() {
        transform.rotation = rotation;
        transform.translation.y = vertical_offset;
    }
}

fn sync_player_visuals(
    parents: Query<&ChildOf, With<PlayerBodyVisual>>,
    player_states: Query<&PlayerVisualState>,
    mut query: Query<(Entity, &mut Transform), With<PlayerBodyVisual>>,
) {
    for (entity, mut transform) in query.iter_mut() {
        let Ok(parent) = parents.get(entity) else {
            continue;
        };
        let Ok(visual_state) = player_states.get(parent.0) else {
            continue;
        };

        if visual_state.crouched {
            transform.translation.y = -0.5 * PLAYER_CROUCH_SCALE;
            transform.scale = Vec3::new(1.0, PLAYER_CROUCH_SCALE, 1.0);
        } else {
            transform.translation.y = -0.5;
            transform.scale = Vec3::ONE;
        }
    }
}

pub fn player_body_mesh(
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
) -> impl Bundle {
    (
        PlayerBodyVisual,
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_xyz(0.0, -0.5, 0.0),
    )
}
