// Keep Bevy's access filters visible in the system signature.
#![allow(clippy::type_complexity)]
use crate::{ClientSystems, sync::ClientSession};
use bevy::{
    camera::visibility::RenderLayers, color::palettes::tailwind, light::NotShadowCaster, prelude::*,
};
use common::{
    Client, Lobby, LocalInput, LocalPlayer, PLAYER_CROUCH_SCALE, PLAYER_CROUCH_VIEW_OFFSET,
    PLAYER_VIEW_HEIGHT, PlayerId, PlayerVisualState, WeaponKind,
};
use common::{combat::SurfaceKind, map::*, raid::EXTRACTION_RADIUS};

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
                sync_local_player_rotation,
                sync_local_view,
                sync_player_visuals,
                sync_local_alive_visibility,
                sync_view_weapon_visibility,
                sync_ammo_hud,
                sync_barrel_laser,
                sync_loot_visuals,
            )
                .in_set(ClientSystems::Present),
        );
    }
}

#[derive(Debug, Component)]
struct WorldModelCamera;

#[derive(Debug, Component)]
struct ViewModelCamera;

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
struct WeaponMeshRoot;

#[derive(Debug, Component)]
struct BarrelLaser;

#[derive(Component)]
struct ViewArm;

#[derive(Component)]
struct LootVisual(usize);

#[derive(Resource)]
struct LootMaterials {
    full: Handle<StandardMaterial>,
    empty: Handle<StandardMaterial>,
}

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
            LocalPlayer,
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
                weapon_view_model(
                    &mut meshes,
                    &mut materials,
                    WeaponKind::Smg,
                    Visibility::Hidden
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
        IsDefaultUiCamera,
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
        ViewModelCamera,
        Camera3d::default(),
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Transform::default(),
        Projection::from(PerspectiveProjection {
            fov: 90.0_f32.to_radians(),
            ..default()
        }),
        RenderLayers::layer(VIEW_MODEL_RENDER_LAYER),
    )
}

fn player_right_arm(arm: Handle<Mesh>, arm_material: Handle<StandardMaterial>) -> impl Bundle {
    let translation = Vec3::new(0.34, -0.2, -0.08);

    (
        ViewArm,
        LocalView,
        BaseLocalOffset(translation),
        Mesh3d(arm),
        MeshMaterial3d(arm_material),
        Transform::from_translation(translation).with_rotation(Quat::from_euler(
            EulerRot::XYZ,
            -0.25,
            0.18,
            -0.55,
        )),
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
    let model_translation = Vec3::from(spec.model_offset);
    let barrel_tip = Vec3::from(spec.barrel_offset) - model_translation;
    let barrel_base = Vec3::new(0.0, 0.03, -0.02);
    let barrel_vector = barrel_tip - barrel_base;
    let barrel_length = barrel_vector.length().max(0.01);
    let barrel_center = barrel_base + barrel_vector * 0.5;
    let barrel_rotation = Quat::from_rotation_arc(Vec3::Z, barrel_vector.normalize_or_zero());
    let barrel_width = match weapon {
        WeaponKind::Rifle | WeaponKind::Smg => 0.08,
        WeaponKind::Pistol => 0.06,
    };
    let body_mesh = meshes.add(Cuboid::new(0.18, 0.12, 0.28));
    let body_material = materials.add(Color::srgb(
        spec.model_color[0],
        spec.model_color[1],
        spec.model_color[2],
    ));
    let grip_mesh = meshes.add(Cuboid::new(0.12, 0.22, 0.1));
    let grip_material = materials.add(Color::srgb(0.1, 0.1, 0.1));
    let barrel_mesh = meshes.add(Cuboid::new(barrel_width, barrel_width * 0.8, barrel_length));
    let barrel_material = materials.add(Color::srgb(
        spec.model_color[0] * 1.1,
        spec.model_color[1] * 1.1,
        spec.model_color[2] * 1.1,
    ));
    let muzzle_mesh = meshes.add(Cuboid::new(barrel_width * 0.85, barrel_width * 0.85, 0.04));
    let muzzle_material = materials.add(Color::srgb(0.05, 0.05, 0.05));
    let laser_mesh = meshes.add(Cuboid::new(0.008, 0.008, BARREL_LASER_LENGTH));
    let laser_material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.0, 0.0),
        emissive: LinearRgba::rgb(18.0, 0.0, 0.0),
        unlit: true,
        ..default()
    });

    (
        LocalView,
        BaseLocalOffset(Vec3::ZERO),
        WeaponViewModel { weapon },
        Transform::default(),
        visibility,
        children![
            (
                WeaponMeshRoot,
                Transform::from_translation(model_translation),
                Visibility::Inherited,
                children![
                    weapon_part(
                        body_mesh,
                        body_material,
                        Transform::from_translation(Vec3::new(0.0, -0.02, 0.02)),
                    ),
                    weapon_part(
                        grip_mesh,
                        grip_material,
                        Transform::from_translation(Vec3::new(0.0, -0.13, 0.08))
                            .with_rotation(Quat::from_rotation_x(-0.35)),
                    ),
                    weapon_part(
                        barrel_mesh,
                        barrel_material,
                        Transform::from_translation(barrel_center).with_rotation(barrel_rotation),
                    ),
                    weapon_part(
                        muzzle_mesh,
                        muzzle_material,
                        Transform::from_translation(barrel_tip).with_rotation(barrel_rotation),
                    ),
                    weapon_kind_extra(meshes, materials, weapon),
                ],
            ),
            (
                BarrelLaser,
                Mesh3d(laser_mesh),
                MeshMaterial3d(laser_material),
                Transform::from_translation(
                    Vec3::from(spec.barrel_offset)
                        + Vec3::new(0.0, 0.0, -BARREL_LASER_LENGTH * 0.5),
                ),
                RenderLayers::layer(VIEW_MODEL_RENDER_LAYER),
                NotShadowCaster,
            ),
        ],
    )
}

fn weapon_kind_extra(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    weapon: WeaponKind,
) -> impl Bundle {
    match weapon {
        WeaponKind::Rifle | WeaponKind::Smg => weapon_part(
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
    commands.insert_resource(ClearColor(Color::srgb(0.18, 0.27, 0.35)));
    for (i, block) in BLOCKS.iter().enumerate() {
        let color = match block.surface {
            SurfaceKind::Wood => Color::srgb(0.48, 0.27, 0.1),
            SurfaceKind::Concrete if i == 0 => Color::srgb(0.2, 0.24, 0.25),
            SurfaceKind::Concrete => Color::srgb(0.43, 0.49, 0.53),
        };
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(Vec3::from(block.half_size) * 2.0))),
            MeshMaterial3d(materials.add(color)),
            Transform::from_translation(Vec3::from(block.center)),
        ));
    }
    let full = materials.add(Color::srgb(1.0, 0.38, 0.045));
    let empty = materials.add(Color::srgb(0.27, 0.25, 0.22));
    for (i, position) in LOOT_POSITIONS.into_iter().enumerate() {
        commands.spawn((
            LootVisual(i),
            Mesh3d(meshes.add(Cuboid::new(1.1, 0.9, 1.1))),
            MeshMaterial3d(full.clone()),
            Transform::from_translation(Vec3::from(position)),
        ));
    }
    commands.insert_resource(LootMaterials { full, empty });
    let green = materials.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.9, 0.35),
        emissive: LinearRgba::rgb(0.0, 2.0, 0.3),
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(EXTRACTION_RADIUS, 0.04))),
        MeshMaterial3d(green.clone()),
        Transform::from_translation(Vec3::from(EXTRACTION_CENTER)),
    ));
    for x in [-EXTRACTION_RADIUS, EXTRACTION_RADIUS] {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.16, 4.0, 0.16))),
            MeshMaterial3d(green.clone()),
            Transform::from_xyz(x, 2.1, EXTRACTION_CENTER[2]),
        ));
    }
}

fn spawn_lights(mut commands: Commands) {
    commands.insert_resource(GlobalAmbientLight {
        brightness: 300.0,
        ..default()
    });
    commands.spawn((
        DirectionalLight {
            illuminance: 12000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-10.0, 15.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::from_layers(&[DEFAULT_RENDER_LAYER, VIEW_MODEL_RENDER_LAYER]),
    ));
}

fn change_fov(
    input: Res<ButtonInput<KeyCode>>,
    mut projections: ParamSet<(
        Single<&mut Projection, With<WorldModelCamera>>,
        Single<&mut Projection, With<ViewModelCamera>>,
    )>,
) {
    let fov = {
        let mut world_projection = projections.p0();
        let Projection::Perspective(perspective) = world_projection.as_mut() else {
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

        perspective.fov
    };

    let mut view_projection = projections.p1();
    let Projection::Perspective(view_model_perspective) = view_projection.as_mut() else {
        unreachable!(
            "The `Projection` component was explicitly built with `Projection::Perspective`"
        );
    };
    view_model_perspective.fov = fov;
}

fn sync_local_view(
    player_input: Res<LocalInput>,
    player_visual_state: Single<&PlayerVisualState, With<LocalPlayer>>,
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
        transform.translation = base_offset.0 + Vec3::Y * (PLAYER_VIEW_HEIGHT + vertical_offset);
    }
}

fn sync_local_player_rotation(
    player_input: Res<LocalInput>,
    mut player_transform: Single<&mut Transform, With<LocalPlayer>>,
) {
    player_transform.rotation = Quat::from_rotation_y(player_input.camera.yaw);
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
            transform.translation.y = 0.0;
            transform.scale = Vec3::new(1.0, PLAYER_CROUCH_SCALE, 1.0);
        } else {
            transform.translation.y = 0.0;
            transform.scale = Vec3::ONE;
        }
    }
}

fn sync_local_alive_visibility(
    player_state: Single<&PlayerVisualState, With<LocalPlayer>>,
    mut overlays: Query<&mut Visibility, Or<(With<Crosshair>, With<AmmoHud>, With<ViewArm>)>>,
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
    player_state: Single<&PlayerVisualState, With<LocalPlayer>>,
    mut query: Query<(&WeaponViewModel, &Children, &mut Visibility)>,
    mut mesh_roots: Query<&mut Visibility, (With<WeaponMeshRoot>, Without<WeaponViewModel>)>,
) {
    for (weapon_view, children, mut visibility) in query.iter_mut() {
        let weapon_visibility = if player_state.alive && weapon_view.weapon == player_state.weapon {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };

        *visibility = weapon_visibility;

        for child in children.iter() {
            let Ok(mut child_visibility) = mesh_roots.get_mut(child) else {
                continue;
            };

            *child_visibility = weapon_visibility;
        }
    }
}

fn sync_ammo_hud(
    player_state: Single<&PlayerVisualState, With<LocalPlayer>>,
    mut query: Query<&mut Text, With<AmmoHud>>,
) {
    for mut text in query.iter_mut() {
        **text = player_state.ammo_in_mag.to_string();
    }
}

fn sync_barrel_laser(
    player_state: Single<&PlayerVisualState, With<LocalPlayer>>,
    weapon_views: Query<&WeaponViewModel>,
    mut lasers: Query<(&ChildOf, &mut Transform, &mut Visibility), With<BarrelLaser>>,
) {
    for (parent, mut transform, mut visibility) in lasers.iter_mut() {
        let Ok(weapon_view) = weapon_views.get(parent.0) else {
            continue;
        };

        let spec = weapon_view.weapon.spec();
        transform.translation =
            Vec3::from(spec.barrel_offset) + Vec3::new(0.0, 0.0, -BARREL_LASER_LENGTH * 0.5);
        *visibility = if player_state.alive && player_state.weapon == weapon_view.weapon {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

pub fn player_body_mesh(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    bot: bool,
) -> impl Bundle {
    let material = materials.add(if bot {
        Color::srgb(0.75, 0.13, 0.1)
    } else {
        Color::srgb(0.1, 0.55, 0.65)
    });
    let head = materials.add(Color::srgb(0.75, 0.61, 0.45));
    let part = |mesh: Handle<Mesh>, material: Handle<StandardMaterial>, translation: Vec3| {
        (
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(translation),
        )
    };
    (
        PlayerBodyVisual,
        Transform::default(),
        Visibility::Inherited,
        children![
            part(
                meshes.add(Cuboid::new(0.46, 0.65, 0.35)),
                material.clone(),
                Vec3::new(0.0, 0.1, 0.0)
            ),
            part(
                meshes.add(Cuboid::new(0.38, 0.35, 0.35)),
                head,
                Vec3::new(0.0, 0.6, 0.0)
            ),
            part(
                meshes.add(Cuboid::new(0.12, 0.55, 0.18)),
                material.clone(),
                Vec3::new(-0.29, 0.05, 0.0)
            ),
            part(
                meshes.add(Cuboid::new(0.12, 0.55, 0.18)),
                material.clone(),
                Vec3::new(0.29, 0.05, 0.0)
            ),
            part(
                meshes.add(Cuboid::new(0.18, 0.5, 0.22)),
                material.clone(),
                Vec3::new(-0.13, -0.55, 0.0)
            ),
            part(
                meshes.add(Cuboid::new(0.18, 0.5, 0.22)),
                material,
                Vec3::new(0.13, -0.55, 0.0)
            ),
        ],
    )
}

fn sync_loot_visuals(
    session: Res<ClientSession>,
    materials: Res<LootMaterials>,
    mut crates: Query<(&LootVisual, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    for (visual, mut material) in &mut crates {
        material.0 = if session.raid.loot_available[visual.0] {
            materials.full.clone()
        } else {
            materials.empty.clone()
        };
    }
}
