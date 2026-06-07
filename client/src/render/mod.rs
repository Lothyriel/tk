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
        let startup_systems = (spawn_view_model, spawn_world_model, spawn_lights, spawn_crosshair);

        app.add_systems(Startup, startup_systems).add_systems(
            Update,
            (
                change_fov,
                sync_local_view,
                sync_player_visuals,
                sync_local_alive_visibility,
            ),
        );
    }
}

#[derive(Debug, Component)]
struct WorldModelCamera;

#[derive(Debug, Component)]
struct LocalView;

#[derive(Debug, Component)]
struct BaseLocalOffset(Vec3);

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

#[derive(Debug, Component)]
struct Crosshair;

const DEFAULT_RENDER_LAYER: usize = 0;
const VIEW_MODEL_RENDER_LAYER: usize = 1;

fn spawn_view_model(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lobby: ResMut<Lobby>,
    player_id: Res<PlayerId>,
) {
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
                view_model_camera(),
                player_right_arm(arm, arm_material),
            ],
            PlayerVisualState {
                alive: true,
                ..default()
            },
        ))
        .id();

    lobby.players.insert(player_id.0, player);
}

fn world_camera() -> impl Bundle {
    (
        LocalView,
        BaseLocalOffset(Vec3::ZERO),
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
        BaseLocalOffset(Vec3::ZERO),
        Camera3d::default(),
        Camera {
            order: 1,
            ..default()
        },
        Transform::default(),
        Projection::from(PerspectiveProjection {
            fov: 70.0_f32.to_radians(),
            ..default()
        }),
        RenderLayers::layer(VIEW_MODEL_RENDER_LAYER),
    )
}

fn player_right_arm(arm: Handle<Mesh>, arm_material: Handle<StandardMaterial>) -> impl Bundle {
    let translation = Vec3::new(0.2, -0.1, -0.25);

    (
        LocalView,
        BaseLocalOffset(translation),
        Mesh3d(arm),
        MeshMaterial3d(arm_material),
        Transform::from_translation(translation),
        RenderLayers::layer(VIEW_MODEL_RENDER_LAYER),
        NotShadowCaster,
    )
}

fn spawn_crosshair(mut commands: Commands) {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            GlobalZIndex(100),
        ))
        .with_child((
            Crosshair,
            Node {
                width: Val::Px(6.0),
                height: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(Color::WHITE),
        ));
}

fn spawn_world_model(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let floor = meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(10.0)));
    let cube = meshes.add(Cuboid::new(2.0, 0.5, 1.0));
    let material = materials.add(Color::WHITE);

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
    mut query: Query<(&BaseLocalOffset, &mut Transform), With<LocalView>>,
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

    for (base_offset, mut transform) in query.iter_mut() {
        transform.rotation = rotation;
        transform.translation = base_offset.0 + Vec3::Y * vertical_offset;
    }
}

fn sync_player_visuals(
    parents: Query<&ChildOf, With<PlayerBodyVisual>>,
    player_states: Query<&PlayerVisualState>,
    mut query: Query<(Entity, &mut Transform, &mut Visibility), With<PlayerBodyVisual>>,
) {
    for (entity, mut transform, mut visibility) in query.iter_mut() {
        let Ok(parent) = parents.get(entity) else {
            continue;
        };
        let Ok(visual_state) = player_states.get(parent.0) else {
            continue;
        };

        *visibility = if visual_state.alive {
            Visibility::Inherited
        } else {
            Visibility::Hidden
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

fn sync_local_alive_visibility(
    player_state: Single<&PlayerVisualState, With<PlayerId>>,
    mut crosshair: Query<&mut Visibility, With<Crosshair>>,
) {
    for mut visibility in crosshair.iter_mut() {
        *visibility = if player_state.alive {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
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
