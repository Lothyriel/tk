// Keep Bevy's access filters visible in the system signature.
#![allow(clippy::type_complexity)]
use crate::{ClientSystems, sync::ClientSession};
use bevy::{camera::visibility::RenderLayers, light::NotShadowCaster, prelude::*};
use common::{
    Client, Lobby, LocalInput, LocalPlayer, PLAYER_CROUCH_VIEW_OFFSET, PLAYER_VIEW_HEIGHT,
    PlayerId, PlayerVisualState, WeaponKind,
};
use common::{combat::SurfaceKind, map::*, raid::EXTRACTION_RADIUS};
pub(crate) mod actors;
mod blood;
mod destruction;
mod dot_sight;
pub(crate) mod feedback;
mod ground_items;
pub(crate) mod item_icons;
mod item_inspection;
pub(crate) mod shot_audio;
pub(crate) mod voxel;
mod weapons;

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewSettings>();
        app.add_plugins(actors::Plugin);
        app.add_plugins(blood::Plugin);
        app.add_plugins(destruction::Plugin);
        app.add_plugins(dot_sight::Plugin);
        app.add_plugins(feedback::Plugin);
        app.add_plugins(shot_audio::Plugin);
        app.add_plugins(ground_items::Plugin);
        app.add_plugins(item_icons::Plugin);
        app.add_plugins(item_inspection::Plugin);
        app.add_plugins((voxel::Plugin, voxel::debug::Plugin));
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
                change_fov.after(feedback::advance),
                fit_view_aspect
                    .after(change_fov)
                    .before(feedback::animate_local),
                sync_local_player_rotation,
                sync_local_view,
                sync_local_alive_visibility,
                ensure_weapon_view
                    .before(feedback::animate_local)
                    .before(sync_view_weapon_visibility),
                sync_view_weapon_visibility.after(feedback::advance),
                sync_ammo_hud,
                sync_barrel_laser.after(feedback::advance),
                sync_loot_visuals,
                sync_barrier_visuals,
            )
                .in_set(ClientSystems::Present),
        );
    }
}

#[derive(Debug, Component)]
pub(crate) struct WorldModelCamera;

#[derive(Debug, Component)]
pub(crate) struct ViewModelCamera;

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
    pub id: u32,
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
struct LootVisual(usize);

#[derive(Component)]
struct BarrierVisual(u16);

fn sync_barrier_visuals(
    session: Res<ClientSession>,
    cover: Option<Res<crate::sync::cover::State>>,
    mut cells: Query<(&BarrierVisual, &mut Visibility)>,
) {
    if !session.is_changed() && cover.as_ref().is_none_or(|state| !state.is_changed()) {
        return;
    }
    for (cell, mut visibility) in &mut cells {
        let removed = cover
            .as_ref()
            .and_then(|state| state.current(&session.raid))
            .is_some_and(|state| {
                state.removed[cell.0 as usize / 8] & (1 << (cell.0 as usize % 8)) != 0
            });
        *visibility = if session.destroyed_cells.contains(&cell.0) || removed {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
}

#[derive(Resource)]
struct LootMaterials {
    full: Handle<StandardMaterial>,
    empty: Handle<StandardMaterial>,
}

const DEFAULT_RENDER_LAYER: usize = 0;
const VIEW_MODEL_RENDER_LAYER: usize = 1;
const BARREL_LASER_LENGTH: f32 = 25.0;

#[derive(Resource)]
struct ViewSettings {
    world_fov: f32,
    current_fov: f32,
    bob_strength: f32,
}
impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            world_fov: 70.0_f32.to_radians(),
            current_fov: 70.0_f32.to_radians(),
            bob_strength: std::env::var("TK_CAMERA_BOB")
                .ok()
                .as_deref()
                .map_or(1.0, camera_bob_strength),
        }
    }
}

fn camera_bob_strength(value: &str) -> f32 {
    match value {
        "off" => 0.0,
        "reduced" => 0.35,
        _ => 1.0,
    }
}

fn stride_bob(phase: f32, speed: f32, ads: f32, strength: f32) -> f32 {
    // Cosmetic translation only; fade to zero at rest and scale down in ADS.
    let amplitude = 0.012 * ((speed - 0.08) / (5.5 - 0.08)).clamp(0.0, 1.0);
    (2.0 * phase).cos() * amplitude * (1.0 - 0.8 * ads.clamp(0.0, 1.0)) * strength
}

fn aimed_fov(base: f32, weapon: WeaponKind, ads: f32) -> f32 {
    let target = base.min(
        if weapon == WeaponKind::Pistol {
            52.0_f32
        } else {
            44.0_f32
        }
        .to_radians(),
    );
    // Interpolate magnification, not raw angle, so zoom has no perceptual lurch.
    let a = (base * 0.5).tan();
    let b = (target * 0.5).tan();
    (a + (b - a) * common::weapon_animation::ease(ads)).atan() * 2.0
}

fn spawn_view_model(mut commands: Commands, mut lobby: ResMut<Lobby>, player_id: Res<PlayerId>) {
    let player = commands
        .spawn((
            Client { id: player_id.0 },
            LocalPlayer,
            Transform::from_xyz(0.0, 1.0, 0.0),
            Visibility::default(),
            children![world_camera(), view_model_camera(),],
            PlayerVisualState {
                alive: true,
                weapon: WeaponKind::Rifle,
                ..default()
            },
        ))
        .id();

    lobby.players.insert(player_id.0, player);
}

/// Keep one presentation container per equipped class; configured contents are
/// replaced atomically by instance/revision, independently of catalog size.
fn ensure_weapon_view(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    session: Res<ClientSession>,
    id: Res<PlayerId>,
    roots: Query<Entity, With<LocalPlayer>>,
    views: Query<&WeaponViewModel>,
) {
    let Some(player) = session.local(id.0) else {
        return;
    };
    if views.iter().any(|view| view.weapon == player.weapon) {
        return;
    }
    let Ok(root) = roots.single() else { return };
    let view = commands
        .spawn(weapon_view_model(
            &mut meshes,
            &mut materials,
            player.weapon,
            Visibility::Hidden,
        ))
        .id();
    commands.entity(root).add_child(view);
}

fn world_camera() -> impl Bundle {
    (
        LocalView,
        BaseLocalOffset(Vec3::ZERO),
        WorldModelCamera,
        bevy::audio::SpatialListener::new(0.18),
        Camera3d::default(),
        Msaa::Off,
        bevy::pbr::ScreenSpaceAmbientOcclusion {
            radius: 0.35,
            ..default()
        },
        IsDefaultUiCamera,
        Transform::default(),
        Projection::from(PerspectiveProjection {
            fov: 70.0_f32.to_radians(),
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
        Msaa::Off,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Transform::default(),
        Projection::from(PerspectiveProjection {
            fov: 65.0_f32.to_radians(),
            ..default()
        }),
        RenderLayers::layer(VIEW_MODEL_RENDER_LAYER),
    )
}

fn weapon_view_model(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    weapon: WeaponKind,
    visibility: Visibility,
) -> impl Bundle {
    let spec = weapon.spec();
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
                Transform::from_translation(view_model_translation(weapon)),
                Visibility::Inherited,
                children![(
                    feedback::Flash,
                    Mesh3d(meshes.add(Cuboid::new(0.075, 0.075, 0.12))),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::srgb(1.0, 0.75, 0.1),
                        emissive: LinearRgba::rgb(14.0, 6.0, 0.5),
                        unlit: true,
                        ..default()
                    })),
                    Transform::default(),
                    Visibility::Hidden,
                    RenderLayers::layer(VIEW_MODEL_RENDER_LAYER),
                    NotShadowCaster,
                )],
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

fn view_model_translation(weapon: WeaponKind) -> Vec3 {
    weapons::view_offset(weapon)
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
    let wood = materials.add(Color::srgb(0.48, 0.27, 0.1));
    let concrete = materials.add(Color::srgb(0.43, 0.49, 0.53));
    let mut cell_meshes: Vec<([f32; 3], Handle<Mesh>)> = Vec::new();
    for cell in barrier_cells() {
        let mesh =
            if let Some((_, mesh)) = cell_meshes.iter().find(|(size, _)| *size == cell.half_size) {
                mesh.clone()
            } else {
                let mesh = meshes.add(Cuboid::from_size(Vec3::from(cell.half_size) * 2.0));
                cell_meshes.push((cell.half_size, mesh.clone()));
                mesh
            };
        commands.spawn((
            BarrierVisual(cell.id),
            Mesh3d(mesh),
            MeshMaterial3d(match cell.surface {
                SurfaceKind::Wood => wood.clone(),
                SurfaceKind::Concrete => concrete.clone(),
            }),
            Transform::from_translation(Vec3::from(cell.center)),
        ));
    }
    for (i, block) in BLOCKS.iter().enumerate() {
        if destructible(i) {
            continue;
        }
        let color = match block.surface {
            SurfaceKind::Wood => Color::srgb(0.48, 0.27, 0.1),
            SurfaceKind::Concrete if i == 0 => Color::srgb(0.2, 0.24, 0.25),
            SurfaceKind::Concrete => Color::srgb(0.43, 0.49, 0.53),
        };
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(Vec3::from(block.half_size) * 2.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: if matches!(block.surface, SurfaceKind::Wood) {
                    0.88
                } else {
                    0.98
                },
                ..default()
            })),
            Transform::from_translation(Vec3::from(block.center)),
        ));
    }
    let full = materials.add(Color::srgb(1.0, 0.38, 0.045));
    for block in pov_fixture()
        .then_some(POV_STEP)
        .into_iter()
        .chain(pov_side_cover())
    {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(Vec3::from(block.half_size) * 2.0))),
            MeshMaterial3d(concrete.clone()),
            Transform::from_translation(Vec3::from(block.center)),
        ));
    }
    if pov_fixture() {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.10, 0.10, 0.012))),
            MeshMaterial3d(full.clone()),
            Transform::from_translation(Vec3::from(common::map::POV_MARKER)),
        ));
    }
    let empty = materials.add(Color::srgb(0.27, 0.25, 0.22));
    for (i, position) in LOOT_POSITIONS.into_iter().enumerate() {
        let band_mesh = meshes.add(Cuboid::new(1.12, 0.06, 1.12));
        let band_material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.16, 0.19, 0.17),
            metallic: 0.7,
            perceptual_roughness: 0.45,
            ..default()
        });
        commands.spawn((
            LootVisual(i),
            Mesh3d(meshes.add(Cuboid::new(1.1, 0.9, 1.1))),
            MeshMaterial3d(full.clone()),
            Transform::from_translation(Vec3::from(position)),
            children![
                (
                    Mesh3d(band_mesh.clone()),
                    MeshMaterial3d(band_material.clone()),
                    Transform::from_xyz(0.0, -0.27, 0.0)
                ),
                (
                    Mesh3d(band_mesh),
                    MeshMaterial3d(band_material),
                    Transform::from_xyz(0.0, 0.27, 0.0)
                )
            ],
        ));
    }
    commands.insert_resource(LootMaterials { full, empty });
    let green = materials.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.9, 0.35),
        emissive: LinearRgba::rgb(0.0, 2.0, 0.3),
        ..default()
    });
    commands.spawn((
        ExtractionMarker,
        Mesh3d(meshes.add(Cylinder::new(EXTRACTION_RADIUS, 0.04))),
        MeshMaterial3d(green.clone()),
        Transform::from_translation(Vec3::from(EXTRACTION_CENTER)),
    ));
    for x in [-EXTRACTION_RADIUS, EXTRACTION_RADIUS] {
        commands.spawn((
            ExtractionMarker,
            Mesh3d(meshes.add(Cuboid::new(0.16, 4.0, 0.16))),
            MeshMaterial3d(green.clone()),
            Transform::from_xyz(x, 2.1, EXTRACTION_CENTER[2]),
        ));
    }
}

/// Non-colliding exit indicators, separable from geometry for diagnostic views.
#[derive(Component)]
pub(crate) struct ExtractionMarker;

fn spawn_lights(mut commands: Commands) {
    // The art bench may isolate material response from the raid's blue fill
    // and hard shadows. Ordinary play keeps its existing illumination.
    let neutral_review = std::env::args().any(|arg| arg == "--playtest")
        && std::env::var_os("TK_PLAYTEST_NEUTRAL_ART").is_some();
    commands.insert_resource(GlobalAmbientLight {
        brightness: 190.0,
        color: if neutral_review {
            Color::WHITE
        } else {
            Color::srgb(0.65, 0.78, 1.0)
        },
        ..default()
    });
    commands.spawn((
        DirectionalLight {
            illuminance: 12000.0,
            color: if neutral_review {
                Color::WHITE
            } else {
                Color::srgb(1.0, 0.89, 0.72)
            },
            shadow_maps_enabled: !neutral_review,
            ..default()
        },
        Transform::from_xyz(-10.0, 15.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::from_layers(&[DEFAULT_RENDER_LAYER, VIEW_MODEL_RENDER_LAYER]),
    ));
}

fn change_fov(
    (input, panel, menu): (
        Res<ButtonInput<KeyCode>>,
        Option<Res<crate::input::inventory_panel::Panel>>,
        Option<Res<crate::menu::Menu>>,
    ),
    time: Res<Time>,
    mut settings: ResMut<ViewSettings>,
    id: Res<PlayerId>,
    session: Res<ClientSession>,
    feedback: Res<feedback::Feedback>,
    mut projections: ParamSet<(
        Single<&mut Projection, With<WorldModelCamera>>,
        Single<&mut Projection, With<ViewModelCamera>>,
    )>,
) {
    {
        let mut world_projection = projections.p0();
        let Projection::Perspective(perspective) = world_projection.as_mut() else {
            unreachable!(
                "The `Projection` component was explicitly built with `Projection::Perspective`"
            );
        };

        let world_zoom = !panel.is_some_and(|panel| panel.open)
            && !menu.is_some_and(|menu| menu.blocks_gameplay());
        if world_zoom && input.pressed(KeyCode::ArrowUp) {
            settings.world_fov -= 45.0_f32.to_radians() * time.delta_secs();
        }
        if world_zoom && input.pressed(KeyCode::ArrowDown) {
            settings.world_fov += 45.0_f32.to_radians() * time.delta_secs();
        }
        settings.world_fov = settings
            .world_fov
            .clamp(20.0_f32.to_radians(), 160.0_f32.to_radians());
        let ads = feedback.responses.get(&id.0).map_or(0.0, |r| r.ads);
        let weapon = feedback
            .responses
            .get(&id.0)
            .and_then(|r| r.displayed)
            .unwrap_or_else(|| session.local(id.0).map_or(WeaponKind::Rifle, |p| p.weapon));
        let target = aimed_fov(settings.world_fov, weapon, ads);
        settings.current_fov +=
            (target - settings.current_fov) * (1.0 - (-time.delta_secs() * 14.0).exp());
        perspective.fov = settings.current_fov;
    }

    let mut view_projection = projections.p1();
    let Projection::Perspective(view_model_perspective) = view_projection.as_mut() else {
        unreachable!(
            "The `Projection` component was explicitly built with `Projection::Perspective`"
        );
    };
    // A stable weapon lens avoids the old fisheye/magnified-stock changes when
    // world zoom changes. ADS aligns actual iron sights on the camera centreline.
    view_model_perspective.fov = 65.0_f32.to_radians();
}

fn fit_view_aspect(
    projection: Single<&Projection, With<ViewModelCamera>>,
    mut roots: Query<
        (
            &ChildOf,
            &mut Transform,
            Option<&voxel::configured::ConfiguredWeapon>,
        ),
        With<WeaponMeshRoot>,
    >,
    weapons: Query<&WeaponViewModel>,
    id: Res<PlayerId>,
    feedback: Res<feedback::Feedback>,
    session: Res<ClientSession>,
) {
    let Projection::Perspective(projection) = &**projection else {
        return;
    };
    for (parent, mut root, configured) in &mut roots {
        if let Ok(weapon) = weapons.get(parent.0) {
            let ads = feedback
                .responses
                .get(&id.0)
                .map_or(0.0, |r| common::weapon_animation::ease(r.ads));
            let spec = configured
                .and_then(|request| {
                    session
                        .configurations
                        .cache
                        .get(request.epoch, request.handle)
                })
                .map_or_else(|| weapon.weapon.spec(), |entry| &entry.resolved);
            if spec
                .assembly
                .anchors
                .contains_key(&common::catalog::AnchorKind::Bore)
            {
                root.translation = spec
                    .assembly
                    .hip_from_root
                    .translation
                    .lerp(spec.assembly.ads_from_root.translation, ads);
                root.rotation = spec
                    .assembly
                    .hip_from_root
                    .rotation
                    .slerp(spec.assembly.ads_from_root.rotation, ads);
            } else {
                root.translation =
                    self::weapons::framed_offset(weapon.weapon, projection.aspect_ratio)
                        .lerp(common::weapon_animation::ads_offset(weapon.weapon), ads);
            }
        }
    }
}

fn sync_local_view(
    player_input: Res<LocalInput>,
    feedback: Option<Res<feedback::Feedback>>,
    id: Option<Res<PlayerId>>,
    presentation: (
        Option<Res<ClientSession>>,
        Option<Res<Time<Real>>>,
        Option<Res<ViewSettings>>,
    ),
    player_visual_state: Single<&PlayerVisualState, With<LocalPlayer>>,
    mut query: Query<(&BaseLocalOffset, &mut Transform), With<LocalView>>,
) {
    let (session, time, view_settings) = presentation;
    let camera = match (feedback, id) {
        (Some(feedback), Some(id)) => feedback.camera(id.0, player_input.camera),
        _ => player_input.camera,
    };
    // Match the timestamps and endpoints used by the local root. The latest
    // stance alone would switch the lens height before the root moves.
    let motion = session
        .zip(time)
        .and_then(|(session, time)| session.local_motion.sample(time.elapsed_secs_f64()));
    let crouch = motion.map_or_else(
        || f32::from(u8::from(player_visual_state.crouched)),
        |pose| pose.crouch,
    );
    let bob = motion.zip(view_settings).map_or(0.0, |(pose, settings)| {
        stride_bob(
            pose.stride_phase,
            pose.stride_speed,
            pose.ads,
            settings.bob_strength,
        )
    });
    let vertical_offset = crouch * PLAYER_CROUCH_VIEW_OFFSET;
    let lean = motion.map_or(player_input.camera.roll, |pose| pose.roll);
    let recoil_roll = camera.roll - player_input.camera.roll;
    let rotation = Quat::from_euler(EulerRot::YXZ, 0.0, camera.pitch, lean + recoil_roll);
    let head_rotation = Quat::from_euler(EulerRot::YXZ, 0.0, player_input.camera.pitch, lean);
    let eye = humanoid_eye(
        common::humanoid::aim_lean_offset(lean)
            + Vec3::Y * (PLAYER_VIEW_HEIGHT + 0.10 + vertical_offset + bob),
        head_rotation,
    );

    for (base_offset, mut transform) in query.iter_mut() {
        transform.rotation = rotation;
        transform.translation = base_offset.0 + eye;
    }
}

/// Cyclopean eye on the front of the voxel head, relative to its central pivot.
fn humanoid_eye(head_center: Vec3, head_rotation: Quat) -> Vec3 {
    head_center + head_rotation * Vec3::new(0.0, 0.025, -0.125)
}

fn sync_local_player_rotation(
    player_input: Res<LocalInput>,
    feedback: Option<Res<feedback::Feedback>>,
    id: Option<Res<PlayerId>>,
    mut player_transform: Single<&mut Transform, With<LocalPlayer>>,
) {
    let camera = match (feedback, id) {
        (Some(feedback), Some(id)) => feedback.camera(id.0, player_input.camera),
        _ => player_input.camera,
    };
    player_transform.rotation = Quat::from_rotation_y(camera.yaw);
}

fn sync_local_alive_visibility(
    player_state: Single<&PlayerVisualState, With<LocalPlayer>>,
    mut overlays: Query<(&mut Visibility, Has<Crosshair>), Or<(With<Crosshair>, With<AmmoHud>)>>,
    id: Res<PlayerId>,
    feedback: Res<feedback::Feedback>,
) {
    let aimed = feedback.responses.get(&id.0).is_some_and(|r| r.ads > 0.8);
    for (mut visibility, crosshair) in overlays.iter_mut() {
        *visibility = if player_state.alive && !(crosshair && aimed) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

fn sync_view_weapon_visibility(
    player_state: Single<&PlayerVisualState, With<LocalPlayer>>,
    session: Res<ClientSession>,
    id: Res<PlayerId>,
    feedback: Res<feedback::Feedback>,
    panel: Option<Res<crate::input::inventory_panel::Panel>>,
    mut query: Query<(&WeaponViewModel, &Children, &mut Visibility)>,
    mut mesh_roots: Query<&mut Visibility, (With<WeaponMeshRoot>, Without<WeaponViewModel>)>,
) {
    let usable = !panel.is_some_and(|panel| panel.inspection.is_some())
        && session.local(id.0).is_none_or(|p| {
            p.body.anatomy.capacity(common::combat::Region::RightArm) > 0.0
                && match p.pose.mode {
                    common::humanoid::PhysicalMode::Fallen => p.pose.mode_time < 0.2,
                    common::humanoid::PhysicalMode::Recovering => p.pose.mode_time > 0.75,
                    _ => true,
                }
        });
    let displayed = feedback
        .responses
        .get(&id.0)
        .and_then(|r| r.displayed)
        .unwrap_or(player_state.weapon);
    for (weapon_view, children, mut visibility) in query.iter_mut() {
        let weapon_visibility = if player_state.alive && usable && weapon_view.weapon == displayed {
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
    id: Res<PlayerId>,
    feedback: Res<feedback::Feedback>,
    session: Res<ClientSession>,
    mut clean_art: Local<Option<bool>>,
) {
    let clean_art = *clean_art.get_or_insert_with(|| {
        std::env::args().any(|arg| arg == "--playtest")
            && std::env::var_os("TK_PLAYTEST_CLEAN_ART").is_some()
    });
    for (parent, mut transform, mut visibility) in lasers.iter_mut() {
        let Ok(weapon_view) = weapon_views.get(parent.0) else {
            continue;
        };

        let ads = feedback.responses.get(&id.0).map_or(0.0, |r| r.ads);
        transform.translation = common::weapon_animation::barrel_offset(weapon_view.weapon, ads)
            + Vec3::new(0.0, 0.0, -BARREL_LASER_LENGTH * 0.5);
        *visibility = if !clean_art
            && player_state.alive
            && player_state.weapon == weapon_view.weapon
            && ads < 0.1
            && session
                .local(id.0)
                .is_none_or(|p| p.reload_remaining() <= 0.0)
        {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

pub fn player_body_mesh(
    _meshes: &mut Assets<Mesh>,
    _materials: &mut Assets<StandardMaterial>,
    _bot: bool,
) -> impl Bundle {
    actors::body()
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

#[cfg(test)]
mod view_tests {
    use super::*;
    #[test]
    fn catalog_view_models_are_instantiated_on_demand_and_reused() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<ClientSession>()
            .insert_resource(PlayerId(7))
            .add_systems(Update, ensure_weapon_view);
        app.world_mut()
            .spawn((LocalPlayer, Transform::default(), Visibility::default()));
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&WeaponViewModel>()
                .iter(app.world())
                .count(),
            0
        );
        app.world_mut()
            .resource_mut::<ClientSession>()
            .players
            .push(common::ClientData {
                id: 7,
                weapon: WeaponKind::Pistol,
                weapon_id: common::WeaponId::M1911,
                ..default()
            });
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&WeaponViewModel>()
                .iter(app.world())
                .count(),
            1
        );
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&WeaponViewModel>()
                .iter(app.world())
                .count(),
            1
        );
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.players[0].weapon = WeaponKind::Rifle;
            session.players[0].weapon_id = common::WeaponId::M4A1;
        }
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&WeaponViewModel>()
                .iter(app.world())
                .count(),
            2
        );
    }

    #[test]
    fn local_camera_stride_bob_is_bounded_and_zero_at_rest_or_when_disabled() {
        use std::f32::consts::FRAC_PI_2;
        let hip = stride_bob(0.0, 5.5, 0.0, 1.0);
        assert!((hip - 0.012).abs() < 0.00001);
        assert!((stride_bob(FRAC_PI_2, 5.5, 0.0, 1.0) + hip).abs() < 0.00001);
        assert!(stride_bob(0.0, 3.25, 0.0, 1.0) < hip);
        assert!(stride_bob(0.0, 5.5, 1.0, 1.0) < hip * 0.21);
        assert!(stride_bob(0.0, 5.5, 0.0, camera_bob_strength("reduced")) < hip * 0.36);
        assert_eq!(stride_bob(0.0, 5.5, 0.0, camera_bob_strength("off")), 0.0);
        for phase in [0.0, FRAC_PI_2] {
            assert_eq!(stride_bob(phase, 0.0, 0.0, 1.0), 0.0);
        }
    }

    #[test]
    fn local_cameras_shift_sideways_with_the_upper_body_lean() {
        let mut app = App::new();
        app.insert_resource(LocalInput(common::ClientInput {
            camera: common::CameraInput {
                roll: 0.3,
                ..default()
            },
            ..default()
        }))
        .add_systems(Update, sync_local_view);
        app.world_mut()
            .spawn((LocalPlayer, PlayerVisualState::default()));
        let world_camera = app
            .world_mut()
            .spawn((LocalView, BaseLocalOffset(Vec3::ZERO), Transform::default()))
            .id();
        let viewmodel_camera = app
            .world_mut()
            .spawn((LocalView, BaseLocalOffset(Vec3::ZERO), Transform::default()))
            .id();

        app.update();

        for camera in [world_camera, viewmodel_camera] {
            let translation = app.world().get::<Transform>(camera).unwrap().translation;
            let expected = Vec3::new(
                -0.21 - 0.025 * 0.3_f32.sin(),
                PLAYER_VIEW_HEIGHT + 0.10 + 0.025 * 0.3_f32.cos(),
                -0.125,
            );
            assert!(translation.distance(expected) < 0.001);
        }
    }

    #[test]
    fn local_cameras_use_confirmed_lean_without_delaying_mouse_look() {
        let mut app = App::new();
        let mut session = ClientSession::default();
        session.local_motion.record(
            1.0,
            crate::sync::LocalMotionPose {
                position: Vec3::ZERO,
                crouch: 1.0,
                stride_phase: 0.0,
                stride_speed: 0.0,
                ads: 0.0,
                roll: 0.22,
                ..default()
            },
            0.0,
            false,
        );
        app.add_plugins(TransformPlugin)
            .insert_resource(session)
            .init_resource::<Time<Real>>()
            .init_resource::<LocalInput>()
            .add_systems(Update, (sync_local_player_rotation, sync_local_view));
        let root = app
            .world_mut()
            .spawn((
                LocalPlayer,
                PlayerVisualState::default(),
                Transform::default(),
            ))
            .id();
        let cameras = [0, 1].map(|_| {
            app.world_mut()
                .spawn((
                    LocalView,
                    BaseLocalOffset(Vec3::ZERO),
                    Transform::default(),
                    ChildOf(root),
                ))
                .id()
        });

        // No fresh packet arrives as the player turns; requested roll has
        // already reversed but neither rendered camera may use that request.
        for (yaw, pitch) in [(0.0, 0.0), (0.38, -0.21)] {
            app.world_mut().resource_mut::<LocalInput>().camera = common::CameraInput {
                yaw,
                pitch,
                roll: -0.3,
            };
            app.update();
            let expected = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.22);
            let eye = Quat::from_rotation_y(yaw)
                * Vec3::new(
                    -0.154,
                    PLAYER_VIEW_HEIGHT + 0.10 + PLAYER_CROUCH_VIEW_OFFSET,
                    0.0,
                )
                + expected * Vec3::new(0.0, 0.025, -0.125);
            for camera in cameras {
                let transform = app.world().get::<GlobalTransform>(camera).unwrap();
                assert!(transform.rotation().dot(expected).abs() > 0.99999);
                assert!(transform.translation().distance(eye) < 0.00001);
            }
        }
    }

    #[test]
    fn budget_fallback_fall_and_getup_lower_sights_restore_fov_and_hide_the_weapon() {
        use common::humanoid::PhysicalMode;
        for mode in [PhysicalMode::Fallen, PhysicalMode::Recovering] {
            let mut app = App::new();
            let mut session = ClientSession::default();
            session.players.push(common::ClientData {
                id: 7,
                alive: true,
                weapon: WeaponKind::Rifle,
                ..default()
            });
            app.insert_resource(session)
                .insert_resource(PlayerId(7))
                .insert_resource(LocalInput(common::ClientInput {
                    aim: true,
                    weapon: common::EquipmentSlot::Primary,
                    ..default()
                }))
                .init_resource::<Time>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<ViewSettings>()
                .init_resource::<feedback::Feedback>()
                .add_systems(
                    Update,
                    (
                        sync_local_view,
                        feedback::advance,
                        change_fov,
                        feedback::animate_local,
                        sync_view_weapon_visibility,
                    )
                        .chain(),
                );
            let world_camera = app
                .world_mut()
                .spawn((
                    WorldModelCamera,
                    LocalView,
                    BaseLocalOffset(Vec3::ZERO),
                    Transform::default(),
                    Projection::from(PerspectiveProjection::default()),
                ))
                .id();
            app.world_mut().spawn((
                ViewModelCamera,
                LocalView,
                BaseLocalOffset(Vec3::ZERO),
                Transform::default(),
                Projection::from(PerspectiveProjection::default()),
            ));
            app.world_mut().spawn((
                LocalPlayer,
                PlayerVisualState {
                    alive: true,
                    weapon: WeaponKind::Rifle,
                    ..default()
                },
            ));
            let mesh = app
                .world_mut()
                .spawn((WeaponMeshRoot, Visibility::Inherited))
                .id();
            let weapon = app
                .world_mut()
                .spawn((
                    WeaponViewModel {
                        weapon: WeaponKind::Rifle,
                    },
                    Visibility::Inherited,
                    LocalView,
                    BaseLocalOffset(Vec3::ZERO),
                    Transform::default(),
                ))
                .add_child(mesh)
                .id();
            for _ in 0..180 {
                app.world_mut()
                    .resource_mut::<Time>()
                    .advance_by(std::time::Duration::from_secs_f32(1.0 / 120.0));
                app.update();
            }
            assert!(app.world().resource::<feedback::Feedback>().responses[&7].ads > 0.99);
            let Projection::Perspective(lens) =
                app.world().get::<Projection>(world_camera).unwrap()
            else {
                panic!("world lens")
            };
            assert!((lens.fov.to_degrees() - 44.0).abs() < 0.1);
            let mut previous = lens.fov;
            {
                let mut session = app.world_mut().resource_mut::<ClientSession>();
                session.players[0].pose.mode = mode;
                session.players[0].pose.mode_time = 0.5;
                assert!(
                    session.players[0].pose.physical.is_none(),
                    "exercise saturated-budget fallback"
                );
            }
            for _ in 0..120 {
                app.update();
                let Projection::Perspective(lens) =
                    app.world().get::<Projection>(world_camera).unwrap()
                else {
                    panic!("world lens")
                };
                assert!(lens.fov + 0.00001 >= previous && lens.fov - previous < 0.035);
                previous = lens.fov;
            }
            assert!(app.world().resource::<feedback::Feedback>().responses[&7].ads < 0.001);
            assert!((previous.to_degrees() - 70.0).abs() < 0.1);
            assert_eq!(
                *app.world().get::<Visibility>(weapon).unwrap(),
                Visibility::Hidden
            );
            assert_eq!(
                *app.world().get::<Visibility>(mesh).unwrap(),
                Visibility::Hidden
            );
            if mode == PhysicalMode::Fallen {
                let mut previous = PLAYER_VIEW_HEIGHT + 0.125;
                for t in [0.0, 0.05, 0.10, 0.15, 0.20] {
                    app.world_mut().resource_mut::<ClientSession>().players[0]
                        .pose
                        .mode_time = t;
                    app.update();
                    let y = app.world().get::<Transform>(weapon).unwrap().translation.y;
                    assert!(
                        y <= previous + 0.0001 && previous - y < 0.13,
                        "fall must lower the actual view weapon continuously"
                    );
                    previous = y;
                }
                assert!((previous - (PLAYER_VIEW_HEIGHT + 0.125 - 0.34)).abs() < 0.0001);
            } else {
                let mut previous = PLAYER_VIEW_HEIGHT + 0.125 - 0.34;
                for t in [0.0, 0.50, 0.75, 0.80, 0.85, 0.90, 0.95, 1.0] {
                    app.world_mut().resource_mut::<ClientSession>().players[0]
                        .pose
                        .mode_time = t;
                    app.update();
                    let y = app.world().get::<Transform>(weapon).unwrap().translation.y;
                    assert!(
                        y + 0.0001 >= previous && y - previous < 0.11,
                        "get-up must raise the actual view weapon continuously"
                    );
                    assert_eq!(
                        *app.world().get::<Visibility>(weapon).unwrap(),
                        if t > 0.75 {
                            Visibility::Inherited
                        } else {
                            Visibility::Hidden
                        }
                    );
                    previous = y;
                }
                assert!((previous - (PLAYER_VIEW_HEIGHT + 0.125)).abs() < 0.0001);
            }
        }
    }
    #[test]
    fn fov_key_speed_is_time_based_instead_of_one_degree_per_render_frame() {
        let simulate = |hz: usize| {
            let mut app = App::new();
            app.init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<Time>()
                .init_resource::<ViewSettings>()
                .init_resource::<ClientSession>()
                .init_resource::<feedback::Feedback>()
                .insert_resource(PlayerId(7))
                .add_systems(Update, change_fov);
            app.world_mut().spawn((
                WorldModelCamera,
                Projection::from(PerspectiveProjection::default()),
            ));
            app.world_mut().spawn((
                ViewModelCamera,
                Projection::from(PerspectiveProjection::default()),
            ));
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::ArrowDown);
            for _ in 0..hz {
                app.world_mut()
                    .resource_mut::<Time>()
                    .advance_by(std::time::Duration::from_secs_f32(1.0 / hz as f32));
                app.update();
            }
            app.world()
                .resource::<ViewSettings>()
                .world_fov
                .to_degrees()
        };
        let a = simulate(60);
        let b = simulate(144);
        assert!((a - 115.0).abs() < 0.01 && (b - a).abs() < 0.01);
    }
    #[test]
    fn ads_zoom_is_continuous_monotonic_and_returns_to_the_selected_world_fov() {
        for weapon in [WeaponKind::Pistol, WeaponKind::Smg, WeaponKind::Rifle] {
            let base = 70.0_f32.to_radians();
            assert!((aimed_fov(base, weapon, 0.0) - base).abs() < 0.0001);
            let expected = if weapon == WeaponKind::Pistol {
                52.0_f32
            } else {
                44.0_f32
            }
            .to_radians();
            assert!((aimed_fov(base, weapon, 1.0) - expected).abs() < 0.0001);
            let mut previous = base;
            for i in 1..=100 {
                let current = aimed_fov(base, weapon, i as f32 / 100.0);
                assert!(current <= previous && previous - current < 0.015);
                previous = current;
            }
        }
    }
    #[test]
    fn both_grips_remain_inside_the_default_first_person_view() {
        for weapon in [WeaponKind::Rifle, WeaponKind::Smg, WeaponKind::Pistol] {
            for grip in [
                Vec3::new(0.025, -0.13, 0.10),
                Vec3::new(-0.07, -0.06, -0.22),
            ] {
                let p = view_model_translation(weapon) + grip;
                assert!(p.z < 0.0);
                assert!(p.y.abs() / (-p.z) < 0.8, "{weapon:?}: {p:?}");
                assert!(p.x.abs() / (-p.z) < 1.5);
            }
        }
    }
}

#[cfg(test)]
mod barrier_tests {
    use super::*;

    #[test]
    fn voxel_barriers_full_state_hides_neighbors_and_restores_after_reset() {
        let mut app = App::new();
        app.init_resource::<ClientSession>()
            .add_systems(Update, sync_barrier_visuals);
        let a = app
            .world_mut()
            .spawn((BarrierVisual(0), Visibility::Inherited))
            .id();
        let b = app
            .world_mut()
            .spawn((BarrierVisual(1), Visibility::Inherited))
            .id();
        // One complete later snapshot is sufficient, even if earlier updates were lost.
        app.world_mut()
            .resource_mut::<ClientSession>()
            .destroyed_cells = vec![0];
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(a).unwrap(),
            Visibility::Hidden
        );
        assert_eq!(
            *app.world().get::<Visibility>(b).unwrap(),
            Visibility::Inherited
        );
        app.world_mut()
            .resource_mut::<ClientSession>()
            .destroyed_cells = vec![0, 1];
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(b).unwrap(),
            Visibility::Hidden
        );
        app.world_mut()
            .resource_mut::<ClientSession>()
            .destroyed_cells
            .clear();
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(a).unwrap(),
            Visibility::Inherited
        );
        assert_eq!(
            *app.world().get::<Visibility>(b).unwrap(),
            Visibility::Inherited
        );
    }
}
