use bevy::{
    camera::visibility::RenderLayers, color::palettes::tailwind, light::NotShadowCaster,
    prelude::*,
};
use common::{
    Client, ClientInput, Lobby, PlayerId, PlayerVisualState, WeaponKind, PLAYER_CROUCH_SCALE,
    PLAYER_CROUCH_VIEW_OFFSET,
};

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        let startup_systems = (
            spawn_view_model,
            spawn_world_model,
            spawn_lights,
            spawn_crosshair,
            spawn_ammo_hud,
        );

        app.add_systems(Startup, startup_systems).add_systems(
            Update,
            (
                change_fov,
                sync_local_view,
                sync_player_visuals,
                sync_local_alive_visibility,
                sync_view_weapon_visibility,
                sync_ammo_hud,
                sync_barrel_laser,
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

#[derive(Debug, Component)]
struct AmmoHud;

#[derive(Debug, Component)]
struct WeaponViewModel {
    weapon: WeaponKind,
}

#[derive(Debug, Component)]
struct BarrelLaser;

const DEFAULT_RENDER_LAYER: usize = 0;
const VIEW_MODEL_RENDER_LAYER: usize = 1;
const BARREL_LASER_LENGTH: f32 = 25.0;

fn spawn_view_model(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lobby: ResMut<Lobby>,
    player_id: Res<PlayerId>,
) {
    let arm = meshes.add(Cuboid::new(0.055, 0.055, 0.34));
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
                weapon_view_model(
                    &mut meshes,
                    &mut materials,
                    WeaponKind::Rifle,
                    Visibility::Inherited,
                ),
                weapon_view_model(
                    &mut meshes,
                    &mut materials,
                    WeaponKind::Pistol,
                    Visibility::Hidden,
                ),
            ],
            PlayerVisualState {
                alive: true,
                weapon: WeaponKind::Rifle,
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
    let translation = Vec3::new(0.34, -0.2, -0.08);

    (
        LocalView,
        BaseLocalOffset(translation),
        Mesh3d(arm),
        MeshMaterial3d(arm_material),
        Transform::from_translation(translation)
            .with_rotation(Quat::from_euler(EulerRot::XYZ, -0.25, 0.18, -0.55)),
        RenderLayers::layer(VIEW_MODEL_RENDER_LAYER),
        NotShadowCaster,
    )
}

fn weapon_view_model(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    weapon: WeaponKind,
    visibility: Visibility,
) -> impl Bundle {
    let spec = weapon.spec();
    let translation = Vec3::from(spec.model_offset);
    let barrel_tip = Vec3::from(spec.barrel_offset) - translation;
    let barrel_base = Vec3::new(0.0, 0.03, -0.02);
    let barrel_vector = barrel_tip - barrel_base;
    let barrel_length = barrel_vector.length().max(0.01);
    let barrel_center = barrel_base + barrel_vector * 0.5;
    let barrel_rotation = Quat::from_rotation_arc(Vec3::Z, barrel_vector.normalize_or_zero());
    let barrel_width = match weapon {
        WeaponKind::Rifle => 0.08,
        WeaponKind::Pistol => 0.06,
    };

    (
        LocalView,
        BaseLocalOffset(translation),
        WeaponViewModel { weapon },
        Transform::from_translation(translation),
        visibility,
        children![
            weapon_part(
                meshes.add(Cuboid::new(0.18, 0.12, 0.28)),
                materials.add(Color::srgb(
                    spec.model_color[0],
                    spec.model_color[1],
                    spec.model_color[2],
                )),
                Transform::from_translation(Vec3::new(0.0, -0.02, 0.02)),
            ),
            weapon_part(
                meshes.add(Cuboid::new(0.12, 0.22, 0.1)),
                materials.add(Color::srgb(0.1, 0.1, 0.1)),
                Transform::from_translation(Vec3::new(0.0, -0.13, 0.08))
                    .with_rotation(Quat::from_rotation_x(-0.35)),
            ),
            weapon_part(
                meshes.add(Cuboid::new(barrel_width, barrel_width * 0.8, barrel_length)),
                materials.add(Color::srgb(
                    spec.model_color[0] * 1.1,
                    spec.model_color[1] * 1.1,
                    spec.model_color[2] * 1.1,
                )),
                Transform::from_translation(barrel_center).with_rotation(barrel_rotation),
            ),
            weapon_part(
                meshes.add(Cuboid::new(barrel_width * 0.85, barrel_width * 0.85, 0.04)),
                materials.add(Color::srgb(0.05, 0.05, 0.05)),
                Transform::from_translation(barrel_tip).with_rotation(barrel_rotation),
            ),
            (
                BarrelLaser,
                Mesh3d(meshes.add(Cuboid::new(0.008, 0.008, BARREL_LASER_LENGTH))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.0, 0.0),
                    emissive: LinearRgba::rgb(18.0, 0.0, 0.0),
                    unlit: true,
                    ..default()
                })),
                Transform::from_translation(
                    barrel_tip + Vec3::new(0.0, 0.0, -BARREL_LASER_LENGTH * 0.5),
                ),
                RenderLayers::layer(VIEW_MODEL_RENDER_LAYER),
                NotShadowCaster,
            ),
            weapon_kind_extra(meshes, materials, weapon),
        ],
    )
}

fn weapon_kind_extra(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    weapon: WeaponKind,
) -> impl Bundle {
    match weapon {
        WeaponKind::Rifle => weapon_part(
            meshes.add(Cuboid::new(0.12, 0.1, 0.34)),
            materials.add(Color::srgb(0.18, 0.12, 0.08)),
            Transform::from_translation(Vec3::new(0.0, -0.03, 0.25))
                .with_rotation(Quat::from_rotation_x(0.18)),
        ),
        WeaponKind::Pistol => weapon_part(
            meshes.add(Cuboid::new(0.12, 0.06, 0.16)),
            materials.add(Color::srgb(0.14, 0.14, 0.16)),
            Transform::from_translation(Vec3::new(0.0, 0.06, -0.1)),
        ),
    }
}

fn weapon_part(
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

fn spawn_ammo_hud(mut commands: Commands) {
    commands.spawn((
        AmmoHud,
        Text::new("30"),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(20.0),
            bottom: Val::Px(18.0),
            ..default()
        },
        TextFont::from_font_size(28.0),
        TextColor(Color::WHITE),
        GlobalZIndex(100),
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
    mut overlays: Query<&mut Visibility, Or<(With<Crosshair>, With<AmmoHud>)>>,
) {
    for mut visibility in overlays.iter_mut() {
        *visibility = if player_state.alive {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

fn sync_view_weapon_visibility(
    player_state: Single<&PlayerVisualState, With<PlayerId>>,
    mut query: Query<(&WeaponViewModel, &mut Visibility)>,
) {
    for (weapon_view, mut visibility) in query.iter_mut() {
        *visibility = if player_state.alive && weapon_view.weapon == player_state.weapon {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

fn sync_ammo_hud(
    player_state: Single<&PlayerVisualState, With<PlayerId>>,
    mut query: Query<&mut Text, With<AmmoHud>>,
) {
    for mut text in query.iter_mut() {
        **text = player_state.ammo_in_mag.to_string();
    }
}

fn sync_barrel_laser(
    player_state: Single<&PlayerVisualState, With<PlayerId>>,
    weapon_views: Query<&WeaponViewModel>,
    mut lasers: Query<(&ChildOf, &mut Transform, &mut Visibility), With<BarrelLaser>>,
) {
    for (parent, mut transform, mut visibility) in lasers.iter_mut() {
        let Ok(weapon_view) = weapon_views.get(parent.0) else {
            continue;
        };

        let spec = weapon_view.weapon.spec();
        let barrel_tip = Vec3::from(spec.barrel_offset) - Vec3::from(spec.model_offset);
        transform.translation = barrel_tip + Vec3::new(0.0, 0.0, -BARREL_LASER_LENGTH * 0.5);
        *visibility = if player_state.alive && player_state.weapon == weapon_view.weapon {
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
