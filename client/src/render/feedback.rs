//! Confirmed shots only. No audio assets, players or sinks on this path.
use super::{LocalView, ViewModelCamera, WeaponMeshRoot, WeaponViewModel, WorldModelCamera};
use crate::{ClientSystems, sync::ClientSession};
use bevy::{platform::collections::HashMap, prelude::*};
use common::weapon_animation::{WeaponAction, WeaponRig, reload_action};
use common::{humanoid::PhysicalMode, *};

// Each pair is (lateral impulse, vertical impulse scale) for one shot in the
// rifle's repeating pattern. Keep values bounded so the spread stays legible.
const RIFLE_RECOIL_PATTERN: [(f32, f32); 8] = [
    (0.00, 1.00),
    (-0.30, 0.94),
    (0.18, 1.08),
    (-0.12, 0.98),
    (0.34, 1.12),
    (-0.24, 0.90),
    (0.10, 1.04),
    (-0.06, 0.96),
];
const RIFLE_RECOIL_PITCH_IMPULSE: f32 = 1.25;
const RIFLE_RECOIL_YAW_IMPULSE: f32 = 0.28;
const RIFLE_RECOIL_LATERAL_JITTER: f32 = 0.035;
const RIFLE_RECOIL_VERTICAL_JITTER: f32 = 0.025;
const CASING_LIFETIME: f32 = 60.0;
const CASING_GROUND_HEIGHT: f32 = 0.105;

#[derive(Debug, Clone, Copy)]
struct CasingSpec {
    radius: f32,
    length: f32,
    ejection_back: f32,
    ejection_side: f32,
}

fn casing_spec(weapon: WeaponKind) -> CasingSpec {
    match weapon {
        WeaponKind::Pistol => CasingSpec {
            radius: 0.0038,
            length: 0.018,
            ejection_back: 0.12,
            ejection_side: 0.045,
        },
        WeaponKind::Smg => CasingSpec {
            radius: 0.0045,
            length: 0.025,
            ejection_back: 0.27,
            ejection_side: 0.055,
        },
        WeaponKind::Rifle => CasingSpec {
            radius: 0.0052,
            length: 0.045,
            ejection_back: 0.40,
            ejection_side: 0.065,
        },
    }
}

fn pattern_jitter(seed: u32) -> f32 {
    let mut bits = seed;
    bits ^= bits >> 16;
    bits = bits.wrapping_mul(0x7feb_352d);
    bits ^= bits >> 15;
    bits = bits.wrapping_mul(0x846c_a68b);
    bits ^= bits >> 16;
    bits as f32 / u32::MAX as f32 * 2.0 - 1.0
}

fn rifle_recoil_pattern(shot_index: u32, shot_id: u64) -> (f32, f32) {
    let (lateral, vertical) =
        RIFLE_RECOIL_PATTERN[shot_index as usize % RIFLE_RECOIL_PATTERN.len()];
    let seed = shot_id as u32 ^ shot_index.rotate_left(13);
    (
        (lateral + pattern_jitter(seed) * RIFLE_RECOIL_LATERAL_JITTER).clamp(-0.4, 0.4),
        (vertical + pattern_jitter(seed.wrapping_add(0x9e37_79b9)) * RIFLE_RECOIL_VERTICAL_JITTER)
            .clamp(0.85, 1.17),
    )
}

pub struct Plugin;
impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Feedback>()
            .add_systems(
                Update,
                (
                    (advance, apply_local_camera_recoil)
                        .chain()
                        .before(super::sync_local_view),
                    animate_local.after(super::sync_local_view),
                    frame_local_pistol.after(animate_local),
                )
                    .in_set(ClientSystems::Present),
            )
            .init_resource::<CasingState>()
            .add_systems(Startup, setup_casing_assets)
            .add_systems(Update, update_casings.in_set(ClientSystems::Present));
    }
}

#[derive(Resource)]
struct CasingAssets {
    meshes: [Handle<Mesh>; 3],
    material: Handle<StandardMaterial>,
}

impl CasingAssets {
    fn mesh(&self, weapon: WeaponKind) -> Handle<Mesh> {
        self.meshes[weapon.index()].clone()
    }
}

fn setup_casing_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut casing_mesh = |weapon| {
        let spec = casing_spec(weapon);
        meshes.add(Cylinder::new(spec.radius, spec.length))
    };
    commands.insert_resource(CasingAssets {
        meshes: [
            casing_mesh(WeaponKind::Rifle),
            casing_mesh(WeaponKind::Pistol),
            casing_mesh(WeaponKind::Smg),
        ],
        material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.68, 0.43, 0.13),
            metallic: 0.72,
            perceptual_roughness: 0.34,
            ..default()
        }),
    });
}

#[derive(Component)]
struct EjectedCasing {
    velocity: Vec3,
    spin: Vec3,
    lifetime: f32,
}

#[derive(Default, Resource)]
struct CasingState {
    raid: Option<u32>,
    last_shot_by_shooter: HashMap<u64, u64>,
    active: Vec<Entity>,
}

impl CasingState {
    fn should_eject(&mut self, shooter: u64, shot_id: u64) -> bool {
        if self
            .last_shot_by_shooter
            .get(&shooter)
            .is_some_and(|last| shot_id <= *last)
        {
            return false;
        }
        self.last_shot_by_shooter.insert(shooter, shot_id);
        true
    }

    fn track(&mut self, entity: Entity) {
        self.active.push(entity);
    }
}

fn step_casing(casing: &mut EjectedCasing, transform: &mut Transform, delta: f32) -> bool {
    casing.lifetime -= delta;
    if casing.lifetime <= 0.0 {
        return true;
    }
    casing.velocity.y -= 9.81 * delta;
    transform.translation += casing.velocity * delta;
    transform.rotation = Quat::from_euler(
        EulerRot::XYZ,
        casing.spin.x * delta,
        casing.spin.y * delta,
        casing.spin.z * delta,
    ) * transform.rotation;
    if transform.translation.y < CASING_GROUND_HEIGHT {
        transform.translation.y = CASING_GROUND_HEIGHT;
        if casing.velocity.y < -0.25 {
            casing.velocity.y *= -0.24;
        } else {
            casing.velocity.y = 0.0;
        }
        casing.velocity.x *= 0.72;
        casing.velocity.z *= 0.72;
        casing.spin *= 0.82;
    }
    false
}

fn update_casings(
    mut commands: Commands,
    time: Res<Time>,
    session: Res<ClientSession>,
    assets: Res<CasingAssets>,
    mut state: ResMut<CasingState>,
    mut casings: Query<(Entity, &mut EjectedCasing, &mut Transform)>,
) {
    if state.raid != Some(session.raid.raid_id) {
        for entity in state.active.drain(..) {
            commands.entity(entity).despawn();
        }
        state.last_shot_by_shooter.clear();
        state.raid = Some(session.raid.raid_id);
    }

    let delta = time.delta_secs().min(0.05);
    let mut expired = Vec::new();
    for (entity, mut casing, mut transform) in &mut casings {
        if step_casing(&mut casing, &mut transform, delta) {
            expired.push(entity);
            commands.entity(entity).despawn();
        }
    }
    state.active.retain(|entity| !expired.contains(entity));

    for shot in &session.casing_shots {
        if !state.should_eject(shot.shooter, shot.id) {
            continue;
        }
        let muzzle = Vec3::from(shot.muzzle);
        let direction = Vec3::from(shot.direction).normalize_or_zero();
        if !muzzle.is_finite() || !direction.is_finite() || direction == Vec3::ZERO {
            continue;
        }
        let right = direction.cross(Vec3::Y).normalize_or_zero();
        if right == Vec3::ZERO {
            continue;
        }
        let up = right.cross(direction).normalize_or_zero();
        let spec = casing_spec(shot.weapon.class());
        let jitter = pattern_jitter(shot.id as u32);
        let position =
            muzzle - direction * spec.ejection_back + right * spec.ejection_side + up * 0.035;
        let velocity =
            right * (1.35 + jitter * 0.35) + up * (1.65 + jitter.abs() * 0.25) + direction * 0.25;
        let entity = commands
            .spawn((
                Mesh3d(assets.mesh(shot.weapon.class())),
                MeshMaterial3d(assets.material.clone()),
                Transform::from_translation(position)
                    .with_rotation(Quat::from_rotation_arc(Vec3::Y, right)),
                EjectedCasing {
                    velocity,
                    spin: Vec3::new(11.0 + jitter * 5.0, 15.0, 8.0 - jitter * 4.0),
                    lifetime: CASING_LIFETIME,
                },
            ))
            .id();
        state.track(entity);
    }
}
#[derive(Default)]
pub(super) struct Response {
    pub recoil: f32,
    velocity: f32,
    recoil_pitch: f32,
    recoil_pitch_velocity: f32,
    recoil_yaw: f32,
    recoil_yaw_velocity: f32,
    rifle_shot_count: u32,
    pub flash: f32,
    last: Option<u64>,
    weapon: Option<WeaponKind>,
    swap: f32,
    pub(super) displayed: Option<WeaponKind>,
    pub(super) action: Option<WeaponAction>,
    sway: Vec2,
    previous_look: Option<(Vec2, f32)>,
    look_rate: Vec2,
    travel: Vec3,
    motion_phase: f32,
    landing: f32,
    pub(super) ads: f32,
    was_alive: bool,
    frame_offset: Vec3,
}
impl Response {
    fn step(&mut self, dt: f32) {
        self.velocity += (-self.recoil * 180.0 - self.velocity * 23.0) * dt;
        self.recoil = (self.recoil + self.velocity * dt).clamp(0.0, 0.11);
        self.recoil_pitch_velocity +=
            (-self.recoil_pitch * 180.0 - self.recoil_pitch_velocity * 23.0) * dt;
        self.recoil_pitch = (self.recoil_pitch + self.recoil_pitch_velocity * dt).clamp(0.0, 0.12);
        self.recoil_yaw_velocity +=
            (-self.recoil_yaw * 180.0 - self.recoil_yaw_velocity * 23.0) * dt;
        self.recoil_yaw = (self.recoil_yaw + self.recoil_yaw_velocity * dt).clamp(-0.035, 0.035);
        self.flash = (self.flash - dt).max(0.0);
        self.swap = (self.swap - dt * 4.0).max(0.0);
        if self.swap <= 0.5 {
            self.displayed = self.weapon;
        }
    }
    fn shot(&mut self, shot: &FiredProjectileData) {
        if self.last.is_some_and(|id| id >= shot.id) {
            return;
        }
        self.last = Some(shot.id);
        self.flash = 0.055;
        let weapon = shot.weapon.class();
        let (lateral, vertical) = if weapon == WeaponKind::Rifle {
            rifle_recoil_pattern(self.rifle_shot_count, shot.id)
        } else {
            (0.0, 1.0)
        };
        let (pitch_impulse, yaw_impulse) = match weapon {
            WeaponKind::Rifle => (
                RIFLE_RECOIL_PITCH_IMPULSE * vertical,
                RIFLE_RECOIL_YAW_IMPULSE * lateral,
            ),
            WeaponKind::Smg => (0.9, 0.0),
            WeaponKind::Pistol => (1.9, 0.0),
        };
        self.recoil_pitch_velocity += pitch_impulse;
        self.recoil_yaw_velocity += yaw_impulse;
        if weapon == WeaponKind::Rifle {
            self.rifle_shot_count = self.rifle_shot_count.wrapping_add(1);
        }
        self.velocity = (self.velocity
            + match weapon {
                WeaponKind::Rifle => 1.35,
                WeaponKind::Smg => 0.9,
                WeaponKind::Pistol => 1.9,
            })
        .min(2.5);
        self.recoil = (self.recoil
            + if weapon == WeaponKind::Rifle {
                0.010
            } else {
                0.012
            })
        .min(0.11);
    }
}
#[derive(Default, Resource)]
pub(super) struct Feedback {
    pub responses: HashMap<u64, Response>,
    raid: u32,
    applied_local_recoil: Vec2,
}
#[derive(Component)]
pub(crate) struct Flash;
#[derive(Component)]
pub(super) struct Bolt;
#[derive(Component)]
pub(super) struct Magazine;
#[derive(Component)]
pub(super) struct SupportHand;
#[derive(Component)]
pub(super) struct Control;
#[derive(Component)]
pub(super) struct Forearm {
    pub support: bool,
}
#[derive(Component)]
pub(super) struct UpperArm {
    pub support: bool,
}

fn forearm_pose(wrist: Vec3, elbow: Vec3) -> Transform {
    let length = wrist.distance(elbow);
    let direction = if length > 0.000001 {
        (wrist - elbow) / length
    } else {
        Vec3::Y
    };
    Transform::from_translation((wrist + elbow) * 0.5)
        .with_rotation(Quat::from_rotation_arc(Vec3::Y, direction))
        .with_scale(Vec3::new(1.0, length / 0.28, 1.0))
}

fn body_side_elbow(wrist: Vec3, body_anchor: Vec3) -> Vec3 {
    // The upper arm is outside this view model. Do not stretch a 28cm forearm
    // all the way to the torso anchor when reaching up to a slide or HK handle.
    wrist + (body_anchor - wrist).clamp_length_max(0.29)
}

fn locomotion_offset(phase: f32, speed: f32, ads: f32) -> Vec3 {
    Vec3::new(phase.sin() * 0.0035, (phase * 2.0).cos() * 0.0015, 0.0)
        * speed.clamp(0.0, 5.5)
        * (1.0 - ads.clamp(0.0, 1.0) * 0.92)
}

pub(super) fn advance(
    time: Res<Time>,
    session: Res<ClientSession>,
    mut feedback: ResMut<Feedback>,
    local_input: Option<Res<LocalInput>>,
    local_id: Option<Res<PlayerId>>,
) {
    if feedback.raid != session.raid.raid_id {
        feedback.responses.clear();
        feedback.raid = session.raid.raid_id;
    }
    for response in feedback.responses.values_mut() {
        response.step(time.delta_secs().min(1.0 / 30.0));
    }
    for player in &session.players {
        let response = feedback.responses.entry(player.id).or_default();
        if response.weapon != Some(player.weapon) {
            response.swap = 1.0;
            if response.displayed.is_none() {
                response.displayed = Some(player.weapon);
            }
            response.weapon = Some(player.weapon);
            response.flash = 0.0;
        }
        if player.alive && !response.was_alive {
            response.swap = 0.5;
            response.displayed = Some(player.weapon);
        }
        response.was_alive = player.alive;
        if !player.alive {
            response.flash = 0.0;
            response.recoil = 0.0;
            response.velocity = 0.0;
            response.recoil_pitch = 0.0;
            response.recoil_pitch_velocity = 0.0;
            response.recoil_yaw = 0.0;
            response.recoil_yaw_velocity = 0.0;
        }
        let w = response.displayed.unwrap_or(player.weapon);
        let fraction = if player.reload_remaining > 0.0 && w == player.weapon {
            1.0 - player.reload_remaining / player.weapon_id.spec().reload_seconds
        } else {
            0.0
        };
        let target = reload_action(w, fraction, player.ammo_in_mag == 0);
        let alpha = 1.0 - (-time.delta_secs().min(0.05) * 32.0).exp();
        let action = response.action.get_or_insert(target);
        action.root.translation += ((target.root.translation - action.root.translation) * alpha)
            .clamp_length_max(1.2 * time.delta_secs().min(0.05));
        action.root.rotation = action.root.rotation.slerp(target.root.rotation, alpha);
        action.support.translation = action
            .support
            .translation
            .lerp(target.support.translation, alpha);
        action.support.rotation = action
            .support
            .rotation
            .slerp(target.support.rotation, alpha);
        action.magazine.translation = action
            .magazine
            .translation
            .lerp(target.magazine.translation, alpha);
        action.magazine.rotation = action
            .magazine
            .rotation
            .slerp(target.magazine.rotation, alpha);
        action.magazine_visible = target.magazine_visible;
        action.bolt += (target.bolt - action.bolt) * alpha;
        action.control += (target.control - action.control) * alpha;
        action.latch += (target.latch - action.latch) * alpha;
        let local = local_id.as_ref().is_some_and(|id| id.0 == player.id);
        let camera = if local {
            local_input
                .as_ref()
                .map_or(player.rot, |input| input.camera)
        } else {
            player.rot
        };
        let look = Vec2::new(camera.yaw, camera.pitch);
        let stamp = if local {
            time.elapsed_secs()
        } else {
            session.simulation_time
        };
        if let Some((old, previous_stamp)) = response.previous_look {
            if stamp > previous_stamp {
                let d = look - old;
                response.look_rate = Vec2::new(d.x.sin().atan2(d.x.cos()), d.y)
                    / (stamp - previous_stamp).max(0.001);
                response.previous_look = Some((look, stamp));
            }
        } else {
            response.previous_look = Some((look, stamp));
        }
        response.sway = response.sway.lerp(
            (response.look_rate * -0.006).clamp(Vec2::splat(-0.035), Vec2::splat(0.035)),
            1.0 - (-time.delta_secs() * 7.0).exp(),
        );
        response.travel = response.travel.lerp(
            player.pose.velocity.into(),
            1.0 - (-time.delta_secs() * 6.0).exp(),
        );
        let speed = Vec2::new(response.travel.x, response.travel.z).length();
        let rate = common::humanoid::stride_phase_rate(speed, player.pose.crouch);
        response.motion_phase =
            (response.motion_phase + rate * time.delta_secs()) % std::f32::consts::TAU;
        response.landing +=
            (player.pose.landing - response.landing) * (1.0 - (-time.delta_secs() * 8.0).exp());
        let requested = if local {
            if local_input
                .as_ref()
                .is_some_and(|i| i.aim && i.weapon == player.weapon_slot)
            {
                1.0
            } else {
                0.0
            }
        } else {
            player.pose.ads
        };
        let usable = player.alive
            && player.pose.physical.is_none()
            && matches!(
                player.pose.mode,
                PhysicalMode::Standing | PhysicalMode::Stumbling
            )
            && player.reload_remaining <= 0.0
            && response.swap <= 0.05
            && player
                .body
                .anatomy
                .capacity(common::combat::Region::RightArm)
                > 0.0
            && !(local_input.as_ref().is_some_and(|i| local && i.run) && speed > 0.5);
        let target = if usable { requested } else { 0.0 };
        response.ads += (target - response.ads) * (1.0 - (-time.delta_secs() * 14.0).exp());
    }
    for shot in &session.shots {
        let response = feedback.responses.entry(shot.shooter).or_default();
        if session
            .local(shot.shooter)
            .is_some_and(|p| p.alive && p.weapon_id == shot.weapon)
        {
            response.shot(shot);
        } else if response.last.is_none_or(|id| shot.id > id) {
            response.last = Some(shot.id);
        }
    }
}

fn apply_recoil_to_camera(camera: &mut CameraInput, applied: &mut Vec2, target: Vec2) {
    let base_yaw = camera.yaw - applied.x;
    camera.yaw = base_yaw + target.x;

    let base_pitch = camera.pitch - applied.y;
    camera.pitch = (base_pitch + target.y).clamp(-1.56, 1.56);
    *applied = Vec2::new(camera.yaw - base_yaw, camera.pitch - base_pitch);
}

fn apply_local_camera_recoil(
    id: Res<PlayerId>,
    mut feedback: ResMut<Feedback>,
    mut input: ResMut<LocalInput>,
) {
    let target = feedback
        .responses
        .get(&id.0)
        .map_or(Vec2::ZERO, |response| {
            Vec2::new(response.recoil_yaw, response.recoil_pitch)
        });
    apply_recoil_to_camera(
        &mut input.camera,
        &mut feedback.applied_local_recoil,
        target,
    );
}

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct LocalAnimationState<'w> {
    time: Res<'w, Time>,
    id: Res<'w, PlayerId>,
    session: Res<'w, ClientSession>,
    feedback: Res<'w, Feedback>,
    input: Res<'w, LocalInput>,
}

pub(super) fn animate_local(
    state: LocalAnimationState,
    mut views: Query<(&WeaponViewModel, &mut Transform), With<LocalView>>,
    mut parts: Query<
        (
            &ChildOf,
            Option<&Flash>,
            Option<&Bolt>,
            Option<&Magazine>,
            Option<&SupportHand>,
            Option<&Control>,
            Option<&Forearm>,
            Option<&UpperArm>,
            &mut Transform,
            &mut Visibility,
        ),
        (
            Without<LocalView>,
            Without<WorldModelCamera>,
            Without<ViewModelCamera>,
        ),
    >,
    roots: Query<&ChildOf, With<WeaponMeshRoot>>,
    weapons: Query<&WeaponViewModel>,
    projection: Single<&Projection, With<ViewModelCamera>>,
    mut cameras: Query<
        &mut Transform,
        (
            Or<(With<WorldModelCamera>, With<ViewModelCamera>)>,
            Without<WeaponViewModel>,
        ),
    >,
) {
    let LocalAnimationState {
        time,
        id,
        session,
        feedback,
        input,
    } = state;
    let Some(p) = session.local(id.0) else {
        return;
    };
    let Some(response) = feedback.responses.get(&id.0) else {
        return;
    };
    let speed = Vec2::new(response.travel.x, response.travel.z).length();
    let phase = response.motion_phase;
    let motion_weight = 1.0 - response.ads * 0.92;
    let displayed = response.displayed.unwrap_or(p.weapon);
    let action = response
        .action
        .unwrap_or_else(|| reload_action(displayed, 0.0, false));
    let swap_drop = if response.swap > 0.5 {
        (1.0 - response.swap) * 2.0
    } else {
        response.swap * 2.0
    };
    let physical_drop = if matches!(p.pose.mode, PhysicalMode::Fallen | PhysicalMode::Recovering) {
        if p.pose.mode == PhysicalMode::Recovering {
            1.0 - common::weapon_animation::ease((p.pose.mode_time - 0.75) / 0.25)
        } else {
            common::weapon_animation::ease(p.pose.mode_time / 0.2)
        }
    } else {
        0.0
    };
    for (weapon, mut t) in &mut views {
        if weapon.weapon != displayed {
            continue;
        }
        let base = *t;
        t.translation += locomotion_offset(phase, speed, response.ads)
            + action.root.translation
            + Vec3::new(response.sway.x * 0.2, -swap_drop * 0.34, response.recoil);
        t.translation +=
            (Vec3::from(p.pose.velocity) - response.travel) * Vec3::new(-0.003, -0.002, -0.004);
        t.translation.y -= response.landing * 0.025 * motion_weight;
        t.rotation *=
            Quat::from_rotation_x((response.travel.y * 0.008).clamp(-0.05, 0.05) * motion_weight);
        let injury = (1.0 - p.body.anatomy.handling()) * (time.elapsed_secs() * 3.7).sin() * 0.035;
        t.rotation *= Quat::from_rotation_x(response.sway.y * motion_weight + swap_drop * 0.6);
        t.rotation *= Quat::from_rotation_y(response.sway.x * motion_weight);
        t.rotation *= Quat::from_rotation_z((injury - response.sway.x * 0.5) * motion_weight);
        if displayed == WeaponKind::Pistol {
            let aspect = if let Projection::Perspective(p) = &**projection {
                p.aspect_ratio
            } else {
                1.0
            };
            let offset = super::weapons::framed_offset(displayed, aspect).lerp(
                common::weapon_animation::ads_offset(displayed),
                common::weapon_animation::ease(response.ads),
            );
            // Cant about the receiver, not the eye. Rotating the 60cm child
            // offset swept the pistol out of the right edge during reload.
            t.translation = base.translation
                + base.rotation * (t.translation - base.translation)
                + t.rotation * (offset - action.root.rotation * offset);
        }
        t.rotation *= action.root.rotation;
        t.translation.y -= physical_drop * 0.34;
        t.rotation *= Quat::from_rotation_x(physical_drop * 0.6);
    }
    for (parent, flash, bolt, mag, hand, control, forearm, upper_arm, mut t, mut visibility) in
        &mut parts
    {
        if flash.is_none()
            && bolt.is_none()
            && mag.is_none()
            && hand.is_none()
            && control.is_none()
            && forearm.is_none()
            && upper_arm.is_none()
        {
            continue;
        }
        let Ok(root_parent) = roots.get(parent.0) else {
            continue;
        };
        let Ok(weapon) = weapons.get(root_parent.0) else {
            continue;
        };
        if weapon.weapon != displayed {
            *visibility = Visibility::Hidden;
            continue;
        }
        if flash.is_some() {
            *visibility = if p.alive && displayed == p.weapon && response.flash > 0.0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        if bolt.is_some() {
            *t = common::weapon_animation::bolt_transform(
                displayed,
                &action,
                response.recoil,
                p.reload_remaining > 0.0,
            );
            if displayed == WeaponKind::Pistol && p.ammo_in_mag == 0 && p.reload_remaining == 0.0 {
                t.translation.z = WeaponRig::for_weapon(displayed).bolt_travel;
            }
            *visibility = Visibility::Inherited;
        }
        if control.is_some() {
            *t = common::weapon_animation::control_transform(displayed, &action);
            *visibility = Visibility::Inherited;
        }
        if mag.is_some() {
            *t = action.magazine;
            *visibility = if action.magazine_visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        if hand.is_some() {
            *visibility = if p.body.anatomy.capacity(common::combat::Region::LeftArm) > 0.0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            *t = action.support;
        }
        if let Some(support) = forearm
            .map(|arm| arm.support)
            .or_else(|| upper_arm.map(|arm| arm.support))
        {
            let aspect = if let Projection::Perspective(p) = &**projection {
                p.aspect_ratio
            } else {
                1.0
            };
            let offset = super::weapons::framed_offset(displayed, aspect).lerp(
                common::weapon_animation::ads_offset(displayed),
                common::weapon_animation::ease(response.ads),
            );
            let grip = if support {
                action.support
            } else {
                Transform::from_translation(WeaponRig::for_weapon(displayed).trigger)
            };
            let wrist = grip.transform_point(Vec3::Z * 0.055);
            let elbow_camera = Vec3::new(if support { -0.26 } else { 0.27 }, -0.34, -0.16);
            let body_anchor =
                action.root.rotation.inverse() * (elbow_camera - action.root.translation) - offset;
            let elbow = body_side_elbow(wrist, body_anchor);
            *t = if forearm.is_some() {
                forearm_pose(wrist, elbow)
            } else {
                forearm_pose(elbow, body_anchor)
            };
            *visibility = if (!support
                || p.body.anatomy.capacity(common::combat::Region::LeftArm) > 0.0)
                && !(upper_arm.is_some() && elbow.distance_squared(body_anchor) < 0.000001)
            {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
    for mut t in &mut cameras {
        let down = fall_blend(&p.pose);
        t.translation.y += camera_height_feedback(&p.pose);
        if p.alive
            && let Some(physical) = &p.pose.physical
        {
            let head = physical.unpack()[common::humanoid::Segment::Head.index()];
            let offset =
                Quat::from_rotation_y(p.rot.yaw - input.camera.yaw) * Vec3::from(head.position);
            t.translation = offset - Vec3::Y * 0.10;
            t.translation.y = t.translation.y.max(0.18 - p.pos[1]);
        }
        let (l, r) = p.body.anatomy.leg_support();
        t.rotation *= Quat::from_rotation_z(
            (r - l) * phase.sin() * 0.018 + p.pose.lean[0] * 0.025 + down * 0.18,
        );
    }
}

fn fall_blend(pose: &common::humanoid::PoseState) -> f32 {
    if pose.mode == PhysicalMode::Recovering {
        1.0 - common::weapon_animation::ease(pose.mode_time)
    } else if matches!(pose.mode, PhysicalMode::Fallen | PhysicalMode::Dead) {
        common::weapon_animation::ease(pose.mode_time / 0.35)
    } else {
        0.0
    }
}
fn camera_height_feedback(pose: &common::humanoid::PoseState) -> f32 {
    pose.root_drop - fall_blend(pose) * (1.15 - pose.crouch * 0.48) - pose.landing * 0.04
}

// Sleeves intentionally leave the bottom of the view; only the manipulated
// weapon, gloves and visible magazine define the presentation envelope.
#[derive(Component, Clone, Copy)]
pub(super) struct ViewFrameBounds(pub [Vec3; 8]);

#[derive(bevy::ecs::system::SystemParam)]
struct LocalFramingState<'w> {
    time: Res<'w, Time>,
    id: Res<'w, PlayerId>,
    session: Res<'w, ClientSession>,
    feedback: ResMut<'w, Feedback>,
}

fn frame_local_pistol(
    state: LocalFramingState,
    camera: Single<
        (&Transform, &Projection),
        (
            With<ViewModelCamera>,
            Without<WeaponViewModel>,
            Without<WeaponMeshRoot>,
            Without<ViewFrameBounds>,
            Without<Forearm>,
            Without<UpperArm>,
        ),
    >,
    mut views: Query<
        (&WeaponViewModel, &mut Transform),
        (
            With<LocalView>,
            Without<ViewModelCamera>,
            Without<WeaponMeshRoot>,
            Without<ViewFrameBounds>,
            Without<Forearm>,
            Without<UpperArm>,
        ),
    >,
    roots: Query<
        (Entity, &ChildOf, &Transform),
        (
            With<WeaponMeshRoot>,
            Without<ViewFrameBounds>,
            Without<Forearm>,
            Without<UpperArm>,
        ),
    >,
    parts: Query<
        (&ChildOf, &Transform, &ViewFrameBounds, &Visibility),
        (
            Without<WeaponMeshRoot>,
            Without<WeaponViewModel>,
            Without<Forearm>,
            Without<UpperArm>,
        ),
    >,
    mut arms: Query<
        (
            &ChildOf,
            Option<&Forearm>,
            Option<&UpperArm>,
            &mut Transform,
        ),
        (
            Or<(With<Forearm>, With<UpperArm>)>,
            Without<WeaponMeshRoot>,
            Without<WeaponViewModel>,
            Without<ViewFrameBounds>,
            Without<ViewModelCamera>,
        ),
    >,
    hands: Query<
        (&ChildOf, &Transform),
        (
            With<SupportHand>,
            Without<WeaponViewModel>,
            Without<WeaponMeshRoot>,
            Without<Forearm>,
            Without<UpperArm>,
        ),
    >,
) {
    let LocalFramingState {
        time,
        id,
        session,
        mut feedback,
    } = state;
    let (eye, projection) = *camera;
    let Projection::Perspective(projection) = projection else {
        return;
    };
    let Some(response) = feedback.responses.get_mut(&id.0) else {
        return;
    };
    if response.displayed != Some(WeaponKind::Pistol) {
        return;
    }
    for (root_entity, root_parent, root) in &roots {
        let Ok((weapon, mut view)) = views.get_mut(root_parent.0) else {
            continue;
        };
        if weapon.weapon != WeaponKind::Pistol {
            continue;
        }
        let composed = view.mul_transform(*root);
        let to_eye = eye.to_matrix().inverse() * composed.to_matrix();
        let points: Vec<_> = parts
            .iter()
            .filter(|(parent, _, _, visibility)| {
                parent.0 == root_entity && **visibility != Visibility::Hidden
            })
            .flat_map(|(_, pose, bounds, _)| {
                bounds
                    .0
                    .map(|p| to_eye.transform_point3(pose.transform_point(p)))
            })
            .collect();
        let lowering = response.swap > 0.05
            || session.local(id.0).is_none_or(|p| {
                !p.alive
                    || !matches!(
                        p.pose.mode,
                        PhysicalMode::Standing | PhysicalMode::Stumbling
                    )
            });
        let correction = if lowering {
            response.frame_offset * (-time.delta_secs() * 14.0).exp()
        } else {
            pistol_frame_correction(&points, projection, response.ads)
        };
        response.frame_offset = correction;
        view.translation += eye.rotation * correction;
        let composed = view.mul_transform(*root);
        let to_local = composed.to_matrix().inverse();
        for (parent, forearm, upper, mut pose) in &mut arms {
            if parent.0 != root_entity {
                continue;
            }
            let support = forearm.map_or_else(|| upper.unwrap().support, |arm| arm.support);
            let grip = if support {
                *hands
                    .iter()
                    .find(|(parent, _)| parent.0 == root_entity)
                    .unwrap()
                    .1
            } else {
                Transform::from_translation(WeaponRig::for_weapon(weapon.weapon).trigger)
            };
            let wrist = grip.transform_point(Vec3::Z * 0.055);
            let anchor = to_local.transform_point3(eye.transform_point(Vec3::new(
                if support { -0.26 } else { 0.27 },
                -0.34,
                -0.16,
            )));
            let elbow = body_side_elbow(wrist, anchor);
            *pose = if forearm.is_some() {
                forearm_pose(wrist, elbow)
            } else {
                forearm_pose(elbow, anchor)
            };
        }
    }
}

fn pistol_frame_correction(points: &[Vec3], projection: &PerspectiveProjection, ads: f32) -> Vec3 {
    if points.is_empty() {
        return Vec3::ZERO;
    }
    let vertical = (projection.fov * 0.5).tan() * 0.94;
    let horizontal = vertical * projection.aspect_ratio;
    let mut left = f32::NEG_INFINITY;
    let mut right = f32::INFINITY;
    let mut bottom = f32::NEG_INFINITY;
    let mut top = f32::INFINITY;
    let mut depth = 0.0_f32;
    for p in points {
        left = left.max(horizontal * p.z - p.x);
        right = right.min(-horizontal * p.z - p.x);
        bottom = bottom.max(vertical * p.z - p.y);
        top = top.min(-vertical * p.z - p.y);
        depth = depth.max(p.z + projection.near + 0.01);
    }
    depth = depth.max((left - right) / (2.0 * horizontal));
    depth = depth.max((bottom - top) / (2.0 * vertical));
    let min = left - horizontal * depth;
    let max = right + horizontal * depth;
    let x = 0.0_f32.max(min).min(max) * (1.0 - common::weapon_animation::ease(ads));
    let y = 0.0_f32
        .max(bottom - vertical * depth)
        .min(top + vertical * depth)
        * (1.0 - common::weapon_animation::ease(ads));
    // ADS may retreat along the sight line, never move the aligned sights off
    // centre just to include the bottom of a glove in the image.
    for p in points {
        depth = depth
            .max(p.z + (p.x + x).abs() / horizontal)
            .max(p.z + (p.y + y).abs() / vertical);
    }
    Vec3::new(x, y, -depth)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_casing_dimensions_increase_from_pistol_to_smg_to_rifle() {
        let pistol = casing_spec(WeaponKind::Pistol);
        let smg = casing_spec(WeaponKind::Smg);
        let rifle = casing_spec(WeaponKind::Rifle);
        assert!(pistol.radius < smg.radius && smg.radius < rifle.radius);
        assert!(pistol.length < smg.length && smg.length < rifle.length);
    }

    #[test]
    fn shell_casing_tracking_has_no_cap_and_lifetime_expires() {
        let mut world = World::new();
        let mut state = CasingState::default();
        let entities: Vec<_> = (0..96).map(|_| world.spawn_empty().id()).collect();
        for entity in &entities {
            state.track(*entity);
        }
        assert_eq!(state.active.len(), entities.len());
        assert_eq!(state.active.first(), entities.first());
        assert_eq!(state.active.last(), entities.last());
        assert_eq!(CASING_LIFETIME, 60.0);

        let mut casing = EjectedCasing {
            velocity: Vec3::ZERO,
            spin: Vec3::ZERO,
            lifetime: CASING_LIFETIME,
        };
        let mut transform = Transform::from_xyz(0.0, 1.0, 0.0);
        assert!(!step_casing(&mut casing, &mut transform, 59.9));
        assert!((casing.lifetime - 0.1).abs() < 0.001);
        assert!(step_casing(&mut casing, &mut transform, 0.2));
    }

    #[test]
    fn shell_casing_dedup_tracks_each_shooter_independently() {
        let mut state = CasingState::default();
        assert!(state.should_eject(7, 10));
        assert!(state.should_eject(8, 9));
        assert!(!state.should_eject(7, 10));
        assert!(!state.should_eject(8, 8));
    }

    #[test]
    fn rifle_recoil_pattern_has_bounded_seeded_variation_and_reduced_impulse() {
        let seeded = rifle_recoil_pattern(0, 1);
        assert_eq!(seeded, rifle_recoil_pattern(0, 1));
        assert_ne!(rifle_recoil_pattern(0, 1), rifle_recoil_pattern(0, 2));
        for (lateral, vertical) in RIFLE_RECOIL_PATTERN {
            assert!((-0.4..=0.4).contains(&lateral));
            assert!((0.85..=1.17).contains(&vertical));
        }
        for shot in 0..64 {
            let (lateral, vertical) = rifle_recoil_pattern(shot, u64::from(shot) * 17);
            assert!((-0.4..=0.4).contains(&lateral));
            assert!((0.85..=1.17).contains(&vertical));
        }

        let mut response = Response::default();
        response.shot(&FiredProjectileData {
            id: 1,
            weapon: WeaponId::M4A1,
            shooter: 7,
            time: 0.0,
            muzzle: [0.0; 3],
            direction: [0.0, 0.0, -1.0],
        });
        assert!((response.recoil - 0.010).abs() < f32::EPSILON);
    }

    #[test]
    fn local_camera_recoil_moves_authoritative_bore_up_and_recovers() {
        let mut app = App::new();
        let mut feedback = Feedback::default();
        let response = feedback.responses.entry(7).or_default();
        response.recoil_pitch = 0.08;
        response.recoil_yaw = 0.02;
        app.insert_resource(feedback)
            .insert_resource(PlayerId(7))
            .init_resource::<LocalInput>()
            .add_systems(Update, apply_local_camera_recoil);

        app.update();
        let camera = &app.world().resource::<LocalInput>().camera;
        let bore = Quat::from(camera) * -Vec3::Z;
        assert!(
            bore.y > 0.0,
            "recoil must raise the authoritative bore: {bore:?}"
        );
        assert!((camera.yaw - 0.02).abs() < 0.001);

        app.world_mut()
            .resource_mut::<Feedback>()
            .responses
            .get_mut(&7)
            .unwrap()
            .recoil_pitch = 0.0;
        app.world_mut()
            .resource_mut::<Feedback>()
            .responses
            .get_mut(&7)
            .unwrap()
            .recoil_yaw = 0.0;
        app.update();
        let camera = &app.world().resource::<LocalInput>().camera;
        assert!(camera.pitch.abs() < 0.001);
        assert!(camera.yaw.abs() < 0.001);
    }

    #[test]
    fn actual_pistol_reload_vertices_stay_inside_the_final_camera_from_portrait_to_ultrawide() {
        use bevy::mesh::VertexAttributeValues;
        let weapon = WeaponKind::Pistol;
        for aspect in [9.0 / 16.0, 4.0 / 3.0, 1.6, 16.0 / 9.0, 21.0 / 9.0] {
            for empty in [false, true] {
                let mut app = App::new();
                let mut session = ClientSession::default();
                session.players.push(ClientData {
                    id: 7,
                    alive: true,
                    weapon,
                    weapon_id: weapon.definition(),
                    weapon_slot: EquipmentSlot::Sidearm,
                    ..default()
                });
                app.insert_resource(session)
                    .init_resource::<Feedback>()
                    .insert_resource(PlayerId(7))
                    .init_resource::<Time>()
                    .init_resource::<LocalInput>()
                    .add_systems(
                        Update,
                        (
                            advance,
                            super::super::fit_view_aspect,
                            animate_local,
                            frame_local_pistol,
                        )
                            .chain(),
                    );
                app.world_mut().resource_mut::<Feedback>().responses.insert(
                    7,
                    Response {
                        weapon: Some(weapon),
                        displayed: Some(weapon),
                        was_alive: true,
                        action: Some(reload_action(weapon, 0.0, empty)),
                        ..default()
                    },
                );
                let projection = PerspectiveProjection {
                    fov: 65.0_f32.to_radians(),
                    aspect_ratio: aspect,
                    ..default()
                };
                let base = Transform::from_xyz(0.0, PLAYER_VIEW_HEIGHT, 0.0)
                    .with_rotation(Quat::from_euler(EulerRot::YXZ, 0.0, 0.85, -0.18));
                let camera = app
                    .world_mut()
                    .spawn((ViewModelCamera, base, Projection::from(projection.clone())))
                    .id();
                let world_camera = app.world_mut().spawn((WorldModelCamera, base)).id();
                let mut meshes = Assets::<Mesh>::default();
                let mut materials = Assets::<StandardMaterial>::default();
                let root = app
                    .world_mut()
                    .spawn((
                        WeaponMeshRoot,
                        super::super::weapons::assembly(
                            &mut meshes,
                            &mut materials,
                            weapon,
                            Vec3::ZERO,
                        ),
                    ))
                    .id();
                let view = app
                    .world_mut()
                    .spawn((LocalView, WeaponViewModel { weapon }, base))
                    .add_child(root)
                    .id();
                let entities: Vec<_> = app
                    .world_mut()
                    .query_filtered::<Entity, With<Mesh3d>>()
                    .iter(app.world())
                    .collect();
                for entity in entities {
                    app.world_mut()
                        .entity_mut(entity)
                        .insert(Visibility::Inherited);
                }
                let mut previous = None;
                // The real 120Hz feedback path includes bounded root travel;
                // injecting raw curves would bypass the presentation filter.
                for i in 0..=300 {
                    let fraction = if i <= 180 { i as f32 / 180.0 } else { 0.0 };
                    app.world_mut()
                        .resource_mut::<Time>()
                        .advance_by(std::time::Duration::from_secs_f32(1.0 / 120.0));
                    {
                        let mut session = app.world_mut().resource_mut::<ClientSession>();
                        session.players[0].ammo_in_mag = if empty && i < 180 { 0 } else { 12 };
                        session.players[0].reload_remaining = if i < 180 {
                            weapon.spec().reload_seconds * (1.0 - fraction)
                        } else {
                            0.0
                        };
                    }
                    app.world_mut().resource_mut::<LocalInput>().aim = i > 180;
                    app.world_mut().resource_mut::<LocalInput>().weapon =
                        if weapon == WeaponKind::Pistol {
                            EquipmentSlot::Sidearm
                        } else {
                            EquipmentSlot::Primary
                        };
                    app.world_mut().entity_mut(camera).insert(base);
                    app.world_mut().entity_mut(world_camera).insert(base);
                    app.world_mut().entity_mut(view).insert(base);
                    app.update();
                    let view_pose = *app.world().get::<Transform>(view).unwrap();
                    let eye = *app.world().get::<Transform>(camera).unwrap();
                    let composed =
                        view_pose.mul_transform(*app.world().get::<Transform>(root).unwrap());
                    let to_eye = eye.to_matrix().inverse() * composed.to_matrix();
                    let mut parts = app.world_mut().query_filtered::<(&Mesh3d, &Transform, &Visibility), With<ViewFrameBounds>>();
                    let mut count = 0;
                    for (mesh, pose, visible) in parts.iter(app.world()) {
                        if *visible == Visibility::Hidden {
                            continue;
                        }
                        let Some(VertexAttributeValues::Float32x3(vertices)) = meshes
                            .get(&mesh.0)
                            .unwrap()
                            .attribute(Mesh::ATTRIBUTE_POSITION)
                        else {
                            panic!("actual vertices required")
                        };
                        for vertex in vertices {
                            let p =
                                to_eye.transform_point3(pose.transform_point(Vec3::from(*vertex)));
                            let vertical = (projection.fov * 0.5).tan();
                            assert!(
                                p.z <= -projection.near,
                                "near-plane clipping aspect={aspect} empty={empty} t={fraction}: {p:?}"
                            );
                            assert!(
                                p.x.abs() <= -p.z * vertical * aspect * 0.941,
                                "horizontal clipping aspect={aspect} empty={empty} t={fraction}: {p:?}"
                            );
                            assert!(
                                p.y.abs() <= -p.z * vertical * 0.941,
                                "vertical clipping aspect={aspect} empty={empty} t={fraction}: {p:?}"
                            );
                            count += 1;
                        }
                    }
                    assert!(
                        count > 1000,
                        "the assembled weapon and both gloves must be checked"
                    );
                    if i <= 180 {
                        if let Some(previous) = previous {
                            assert!(
                                view_pose.translation.distance(previous) < 0.035,
                                "framing must not snap as the magazine changes visibility: aspect={aspect} empty={empty} t={fraction} before={previous:?} after={:?}",
                                view_pose.translation
                            );
                        }
                        previous = Some(view_pose.translation);
                    }
                    if i == 300 {
                        let sight = to_eye.transform_point3(Vec3::new(0.0, 0.118, 0.005));
                        assert!(
                            sight.x.abs() < 0.0001 && sight.y.abs() < 0.0001,
                            "framing must preserve centred ADS: {sight:?}"
                        );
                        let Projection::Perspective(actual_projection) =
                            app.world().get::<Projection>(camera).unwrap()
                        else {
                            panic!("perspective camera required")
                        };
                        assert_eq!(
                            actual_projection.fov, projection.fov,
                            "framing must not change the lens"
                        );
                    }
                }
                let before = app.world().resource::<Feedback>().responses[&7].frame_offset;
                app.world_mut().resource_mut::<ClientSession>().players[0]
                    .pose
                    .mode = PhysicalMode::Fallen;
                app.world_mut().resource_mut::<ClientSession>().players[0]
                    .pose
                    .mode_time = 0.3;
                app.world_mut()
                    .resource_mut::<Time>()
                    .advance_by(std::time::Duration::from_secs_f32(1.0 / 120.0));
                app.world_mut().entity_mut(camera).insert(base);
                app.world_mut().entity_mut(world_camera).insert(base);
                app.world_mut().entity_mut(view).insert(base);
                app.update();
                let after = app.world().resource::<Feedback>().responses[&7].frame_offset;
                assert!(
                    after.distance(before * (-14.0_f32 / 120.0).exp()) < 0.00001,
                    "falling must ease out framing, never recenter a deliberately lowered weapon"
                );
            }
        }
    }

    #[test]
    fn composed_view_forearms_keep_anatomical_length_and_actual_glove_contact_in_ads_and_reload() {
        for weapon in [WeaponKind::Pistol, WeaponKind::Smg, WeaponKind::Rifle] {
            for ads in [0.0, 0.5, 1.0] {
                for fraction in [0.0, 0.10, 0.55, 0.82, 0.94] {
                    let mut app = App::new();
                    let action = reload_action(weapon, fraction, true);
                    let mut session = ClientSession::default();
                    session.players.push(ClientData {
                        id: 7,
                        alive: true,
                        weapon,
                        weapon_id: weapon.definition(),
                        weapon_slot: if weapon == WeaponKind::Pistol {
                            EquipmentSlot::Sidearm
                        } else {
                            EquipmentSlot::Primary
                        },
                        reload_remaining: if fraction > 0.0 {
                            weapon.spec().reload_seconds * (1.0 - fraction)
                        } else {
                            0.0
                        },
                        ..default()
                    });
                    let mut feedback = Feedback::default();
                    feedback.responses.insert(
                        7,
                        Response {
                            displayed: Some(weapon),
                            action: Some(action),
                            ads,
                            recoil: 0.025,
                            ..default()
                        },
                    );
                    app.insert_resource(session)
                        .insert_resource(feedback)
                        .insert_resource(PlayerId(7))
                        .init_resource::<Time>()
                        .init_resource::<LocalInput>()
                        .add_systems(Update, (animate_local, frame_local_pistol).chain());
                    app.world_mut().spawn((
                        ViewModelCamera,
                        Transform::default(),
                        Projection::from(PerspectiveProjection {
                            fov: 65.0_f32.to_radians(),
                            aspect_ratio: 1.6,
                            ..default()
                        }),
                    ));
                    app.world_mut()
                        .spawn((WorldModelCamera, Transform::default()));
                    let offset = super::super::weapons::view_offset(weapon).lerp(
                        common::weapon_animation::ads_offset(weapon),
                        common::weapon_animation::ease(ads),
                    );
                    let mut meshes = Assets::<Mesh>::default();
                    let mut materials = Assets::<StandardMaterial>::default();
                    let root = app
                        .world_mut()
                        .spawn((
                            WeaponMeshRoot,
                            super::super::weapons::assembly(
                                &mut meshes,
                                &mut materials,
                                weapon,
                                offset,
                            ),
                        ))
                        .id();
                    let view = app
                        .world_mut()
                        .spawn((LocalView, WeaponViewModel { weapon }, Transform::default()))
                        .add_child(root)
                        .id();
                    // DefaultPlugins registers visibility for rendered meshes;
                    // this GPU-free app installs that component explicitly.
                    let parts: Vec<_> = app
                        .world_mut()
                        .query_filtered::<Entity, With<Mesh3d>>()
                        .iter(app.world())
                        .collect();
                    for part in parts {
                        app.world_mut()
                            .entity_mut(part)
                            .insert(Visibility::Inherited);
                    }
                    app.update();
                    let parent = *app.world().get::<Transform>(view).unwrap();
                    let mesh_root = *app.world().get::<Transform>(root).unwrap();
                    let composed = parent.mul_transform(mesh_root);
                    // ANIM-14: a mechanically correct contact is not enough if
                    // the sleeves hide the manipulated slide from the eye.
                    if weapon == WeaponKind::Pistol && ads == 0.0 && fraction == 0.82 {
                        use common::humanoid::{SegmentPose, ray_box};
                        let slide = {
                            let mut slides =
                                app.world_mut().query_filtered::<&Transform, With<Bolt>>();
                            *slides.single(app.world()).unwrap()
                        };
                        let mut sleeves = app.world_mut().query_filtered::<(
                            &Transform,
                            &Mesh3d,
                            &Visibility,
                            Option<&Forearm>,
                            Option<&UpperArm>,
                        ), Or<(With<Forearm>, With<UpperArm>)>>(
                        );
                        let sleeves: Vec<_> = sleeves
                            .iter(app.world())
                            .filter(|(_, _, visible, _, _)| **visible != Visibility::Hidden)
                            .map(|(pose, mesh, _, forearm, upper)| {
                                let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) =
                                    meshes
                                        .get(&mesh.0)
                                        .unwrap()
                                        .attribute(Mesh::ATTRIBUTE_POSITION)
                                else {
                                    panic!("actual sleeve vertices required")
                                };
                                let min = vertices
                                    .iter()
                                    .map(|v| Vec3::from(*v))
                                    .fold(Vec3::splat(f32::INFINITY), Vec3::min);
                                let max = vertices
                                    .iter()
                                    .map(|v| Vec3::from(*v))
                                    .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
                                let world = composed.mul_transform(*pose);
                                (
                                    if forearm.is_some() {
                                        "forearm"
                                    } else {
                                        "upper"
                                    },
                                    forearm
                                        .map_or_else(|| upper.unwrap().support, |arm| arm.support),
                                    SegmentPose {
                                        position: world.translation.into(),
                                        rotation: world.rotation.to_array(),
                                    },
                                    (max - min) * 0.5 * world.scale,
                                )
                            })
                            .collect();
                        let facing_side = (composed.rotation.inverse() * -composed.translation)
                            .x
                            .signum()
                            * 0.035;
                        let occlusions: Vec<_> = [-0.16, -0.09, -0.02]
                            .into_iter()
                            .map(|z| {
                                let rail = composed.transform_point(
                                    slide.transform_point(Vec3::new(facing_side, 0.064, z)),
                                );
                                let hits: Vec<_> = sleeves
                                    .iter()
                                    .filter_map(|(kind, support, pose, half)| {
                                        ray_box(
                                            Vec3::ZERO,
                                            rail.normalize(),
                                            *pose,
                                            *half,
                                            rail.length() - 0.001,
                                        )
                                        .map(|hit| (*kind, *support, hit.0, *pose))
                                    })
                                    .collect();
                                (z, rail, hits)
                            })
                            .collect();
                        let clear = occlusions
                            .iter()
                            .filter(|(_, _, hits)| hits.is_empty())
                            .count();
                        assert!(
                            clear >= 2,
                            "sleeves obscure the actual manipulated slide face: {clear}/3 rail samples visible; {occlusions:?}"
                        );
                    }
                    let (glove_mesh, support_pose) = {
                        let mut hands = app
                            .world_mut()
                            .query_filtered::<(&Mesh3d, &Transform), With<SupportHand>>();
                        let (mesh, pose) = hands.single(app.world()).unwrap();
                        (mesh.0.clone(), *pose)
                    };
                    let trigger_pose = {
                        let mut hands = app
                            .world_mut()
                            .query_filtered::<(&Mesh3d, &Transform), Without<SupportHand>>();
                        *hands
                            .iter(app.world())
                            .find(|(mesh, _)| mesh.0 == glove_mesh)
                            .unwrap()
                            .1
                    };
                    let uppers: Vec<_> = app
                        .world_mut()
                        .query::<(&UpperArm, &Transform)>()
                        .iter(app.world())
                        .map(|(arm, pose)| (arm.support, *pose))
                        .collect();
                    let mut arms = app.world_mut().query::<(&Forearm, &Transform, &Mesh3d)>();
                    assert_eq!(arms.iter(app.world()).count(), 2);
                    for (forearm, pose, mesh) in arms.iter(app.world()) {
                        let grip = if forearm.support {
                            support_pose
                        } else {
                            trigger_pose
                        };
                        let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) = meshes
                            .get(&mesh.0)
                            .unwrap()
                            .attribute(Mesh::ATTRIBUTE_POSITION)
                        else {
                            panic!("actual sleeve mesh vertices required");
                        };
                        let top = vertices
                            .iter()
                            .map(|p| p[1])
                            .fold(f32::NEG_INFINITY, f32::max);
                        let bottom = vertices.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
                        let wrist = composed.transform_point(grip.transform_point(Vec3::Z * 0.055));
                        let cuff = composed.transform_point(pose.transform_point(Vec3::Y * top));
                        let elbow =
                            composed.transform_point(pose.transform_point(Vec3::Y * bottom));
                        assert!(
                            cuff.distance(wrist) < 0.0001,
                            "actual rendered cuff detached from glove: {weapon:?} ads={ads} reload={fraction} support={} cuff={cuff:?} wrist={wrist:?} local={pose:?}",
                            forearm.support
                        );
                        assert!(
                            cuff.distance(elbow) <= 0.291,
                            "view arm stretched across the mechanism"
                        );
                        assert!(pose.rotation.is_finite() && elbow.is_finite());
                        let upper = uppers
                            .iter()
                            .find(|(support, _)| *support == forearm.support)
                            .unwrap()
                            .1;
                        let upper_end =
                            composed.transform_point(upper.transform_point(Vec3::Y * 0.14));
                        assert!(
                            upper_end.distance(elbow) < 0.0001,
                            "visible elbow must join the upper arm, never float"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn forearm_stays_on_the_wrist_and_body_side_instead_of_rotating_with_the_reload_glove() {
        for weapon in [WeaponKind::Pistol, WeaponKind::Smg, WeaponKind::Rifle] {
            for i in 0..=100 {
                let action = reload_action(weapon, i as f32 / 100.0, true);
                let offset = super::super::weapons::view_offset(weapon);
                let wrist = action.support.transform_point(Vec3::Z * 0.055);
                let elbow = action.root.rotation.inverse()
                    * (Vec3::new(-0.26, -0.34, -0.16) - action.root.translation)
                    - offset;
                let sleeve = forearm_pose(wrist, elbow);
                assert!(sleeve.transform_point(Vec3::Y * 0.14).distance(wrist) < 0.0001);
                assert!(sleeve.transform_point(-Vec3::Y * 0.14).distance(elbow) < 0.0001);
                let body_end = action.root.transform_point(offset + elbow);
                // action.root rotates the child offset as well as the sleeve.
                assert!(body_end.x < -0.20 && body_end.y < -0.25);
            }
        }
    }
    #[test]
    fn movement_is_centimetre_scale_continuous_and_quieter_on_the_sights() {
        let rate = common::humanoid::stride_phase_rate(5.5, 0.0);
        let mut previous = locomotion_offset(0.0, 5.5, 0.0);
        for i in 1..=240 {
            let phase = i as f32 / 120.0 * rate;
            let p = locomotion_offset(phase, 5.5, 0.0);
            assert!(p.length() < 0.022);
            assert!(p.distance(previous) < 0.003);
            assert!(locomotion_offset(phase, 5.5, 1.0).length() < 0.002);
            previous = p;
        }
    }
    #[test]
    fn snapshot_look_updates_hold_a_measured_turn_rate_without_sawtooth_weapon_sway() {
        let mut app = App::new();
        let mut session = ClientSession::default();
        session.players.push(ClientData {
            id: 7,
            alive: true,
            ..default()
        });
        app.insert_resource(session)
            .init_resource::<Time>()
            .init_resource::<Feedback>()
            .add_systems(Update, advance);
        let mut samples = Vec::new();
        for frame in 0..120 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(1.0 / 120.0));
            if frame % 4 == 0 {
                let mut session = app.world_mut().resource_mut::<ClientSession>();
                session.simulation_time = frame as f32 / 120.0;
                session.players[0].rot.yaw = session.simulation_time;
            }
            app.update();
            if frame > 90 {
                samples.push(app.world().resource::<Feedback>().responses[&7].sway.x);
            }
        }
        let min = samples.iter().copied().fold(f32::INFINITY, f32::min);
        let max = samples.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(min > -0.0075 && max < -0.0045);
        assert!(
            max - min < 0.001,
            "network sampling must not pulse the weapon every render frame"
        );
    }
    #[test]
    fn swap_holsters_before_revealing_new_model_and_reload_interrupt_blends_from_current_pose() {
        let mut app = App::new();
        let mut session = ClientSession::default();
        session.players.push(ClientData {
            id: 7,
            alive: true,
            weapon: WeaponKind::Pistol,
            weapon_id: WeaponId::M1911,
            weapon_slot: EquipmentSlot::Sidearm,
            ammo_in_mag: 12,
            reload_remaining: 0.75,
            ..default()
        });
        app.init_resource::<Time>()
            .init_resource::<Feedback>()
            .insert_resource(session)
            .add_systems(Update, advance);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(1.0 / 120.0));
        app.update();
        let before = app.world().resource::<Feedback>().responses[&7]
            .action
            .unwrap();
        app.world_mut().resource_mut::<ClientSession>().players[0].reload_remaining = 0.0;
        app.update();
        let after = app.world().resource::<Feedback>().responses[&7]
            .action
            .unwrap();
        assert!(after.root.translation.distance(before.root.translation) < 0.02);
        assert!(
            after
                .support
                .translation
                .distance(before.support.translation)
                < 0.08
        );
        assert!(
            after.root.translation.length() > 0.01,
            "interruption must not snap straight to idle"
        );
        app.world_mut().resource_mut::<ClientSession>().players[0].weapon = WeaponKind::Rifle;
        app.world_mut().resource_mut::<ClientSession>().players[0].weapon_id = WeaponId::M4A1;
        app.update();
        assert_eq!(
            app.world().resource::<Feedback>().responses[&7].displayed,
            Some(WeaponKind::Pistol)
        );
        for _ in 0..16 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<Feedback>().responses[&7].displayed,
            Some(WeaponKind::Rifle)
        );
        assert_eq!(
            app.world().resource::<ClientSession>().players[0].ammo_in_mag,
            12,
            "presentation cannot grant ammo"
        );
    }
    #[test]
    fn confirmed_shot_is_once_bounded_and_recovers() {
        let mut r = Response::default();
        let shot = FiredProjectileData {
            id: 1,
            weapon: WeaponId::M4A1,
            shooter: 7,
            time: 0.0,
            muzzle: [0.0; 3],
            direction: [0.0, 0.0, -1.0],
        };
        r.shot(&shot);
        let v = r.velocity;
        r.shot(&shot);
        assert_eq!(r.velocity, v);
        assert!(r.flash > 0.0);
        for i in 2..100 {
            r.shot(&FiredProjectileData {
                id: i,
                ..shot.clone()
            });
            r.step(0.01);
            assert!(r.recoil <= 0.11);
        }
        for _ in 0..240 {
            r.step(1.0 / 120.0);
        }
        assert!(r.recoil < 0.001);
        assert_eq!(r.flash, 0.0);
    }
    #[test]
    fn standing_and_crouched_falls_keep_camera_above_floor_without_root_jump() {
        use common::humanoid::*;
        for (crouch, root, drop) in [(0.0, 1.0, 0.6), (1.0, 0.7, 0.3)] {
            let mut pose = PoseState {
                crouch,
                root_drop: drop,
                mode: PhysicalMode::Fallen,
                ..default()
            };
            let view = PLAYER_VIEW_HEIGHT + crouch * PLAYER_CROUCH_VIEW_OFFSET;
            assert!(
                ((root - drop) + view + camera_height_feedback(&pose) - (root + view)).abs()
                    < 0.001
            );
            pose.mode_time = 1.0;
            assert!(((root - drop) + view + camera_height_feedback(&pose) - 0.4).abs() < 0.001);
            pose.mode = PhysicalMode::Recovering;
            pose.mode_time = 0.0;
            pose.root_drop = 0.0;
            assert!((root + view + camera_height_feedback(&pose) - 0.4).abs() < 0.001);
        }
    }
    #[test]
    fn first_confirmed_shot_on_a_new_weapon_flashes_but_old_weapon_never_replays() {
        let mut app = App::new();
        let mut session = ClientSession::default();
        session.players.push(ClientData {
            id: 7,
            alive: true,
            weapon: WeaponKind::Pistol,
            weapon_id: WeaponId::M1911,
            weapon_slot: EquipmentSlot::Sidearm,
            ..default()
        });
        session.shots.push(FiredProjectileData {
            id: 1,
            shooter: 7,
            weapon: WeaponId::M1911,
            time: 0.0,
            muzzle: [0.0; 3],
            direction: [0.0, 0.0, -1.0],
        });
        app.init_resource::<Time>()
            .init_resource::<Feedback>()
            .insert_resource(session)
            .add_systems(Update, advance);
        app.update();
        assert!(app.world().resource::<Feedback>().responses[&7].flash > 0.0);
        app.world_mut().resource_mut::<ClientSession>().players[0].weapon = WeaponKind::Rifle;
        app.world_mut().resource_mut::<ClientSession>().players[0].weapon_id = WeaponId::M4A1;
        app.update();
        assert_eq!(app.world().resource::<Feedback>().responses[&7].flash, 0.0);
        app.world_mut().resource_mut::<ClientSession>().players[0].weapon = WeaponKind::Pistol;
        app.world_mut().resource_mut::<ClientSession>().players[0].weapon_id = WeaponId::M1911;
        app.update();
        assert_eq!(app.world().resource::<Feedback>().responses[&7].flash, 0.0);
    }
}
