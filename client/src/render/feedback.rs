//! Visual feedback for confirmed shots and weapon handling.
use super::{LocalView, ViewModelCamera, WeaponMeshRoot, WeaponViewModel, WorldModelCamera};
use crate::{ClientSystems, sync::ClientSession};
use bevy::{platform::collections::HashMap, prelude::*};
#[cfg(test)]
use common::weapon_animation::WeaponRig;
use common::weapon_animation::{WeaponAction, reload_action};
use common::{humanoid::PhysicalMode, *};
use std::collections::VecDeque;

mod configured_local;
mod recoil;

// Cosmetic only. Bound the render/update cost across all shooters in the raid.
const CASING_LIFETIME: f32 = 20.0;
const MAX_ACTIVE_CASINGS: usize = 128;
const CASING_SURFACE_CLEARANCE: f32 = 0.005;

#[derive(Debug, Clone, Copy)]
struct CasingSpec {
    radius: f32,
    length: f32,
    ejection_back: f32,
    ejection_side: f32,
}

fn casing_jitter(mut bits: u32) -> f32 {
    bits ^= bits >> 16;
    bits = bits.wrapping_mul(0x7feb_352d);
    bits ^= bits >> 15;
    bits = bits.wrapping_mul(0x846c_a68b);
    bits ^= bits >> 16;
    bits as f32 / u32::MAX as f32 * 2.0 - 1.0
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

pub struct Plugin;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PrepareLocal;
impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Feedback>()
            .add_systems(
                Update,
                (
                    advance
                        .before(super::sync_local_view)
                        .before(super::sync_local_player_rotation),
                    animate_local.after(super::sync_local_view),
                    configured_local::prepare
                        .after(animate_local)
                        .in_set(PrepareLocal),
                )
                    .in_set(ClientSystems::Present),
            )
            .init_resource::<CasingState>()
            .add_systems(Startup, setup_casing_assets)
            .add_systems(
                Update,
                configured_local::flash
                    .after(super::voxel::configured::PoseConfigured)
                    .after(super::sync_view_weapon_visibility)
                    .in_set(ClientSystems::Present),
            )
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
    shots_by_shooter: HashMap<u64, common::replication::shots::ShotWindow>,
    active: VecDeque<Entity>,
}

impl CasingState {
    fn should_eject(&mut self, shooter: u64, shot_id: u64) -> bool {
        self.shots_by_shooter
            .entry(shooter)
            .or_default()
            .observe(shot_id)
    }

    fn track(&mut self, entity: Entity) -> Option<Entity> {
        let evicted = if self.active.len() == MAX_ACTIVE_CASINGS {
            self.active.pop_front()
        } else {
            None
        };
        self.active.push_back(entity);
        evicted
    }
}

struct CasingSurfaces {
    cells: Vec<map::BarrierCell>,
    fixtures: Vec<map::MapBlock>,
}

impl CasingSurfaces {
    fn new() -> Self {
        Self {
            cells: map::barrier_cells(),
            fixtures: map::pov_blocks().collect(),
        }
    }
}

/// Earliest entry of a cosmetic casing center into an intact map box.
/// A casing already inside a box can leave it instead of sticking to it.
fn casing_box_entry(from: Vec3, delta: Vec3, center: Vec3, half: Vec3) -> Option<(f32, Vec3)> {
    let local = from - center;
    let mut entry = f32::NEG_INFINITY;
    let mut exit = 1.0_f32;
    let mut normal = Vec3::ZERO;
    for axis in 0..3 {
        if delta[axis] == 0.0 {
            if local[axis].abs() > half[axis] {
                return None;
            }
            continue;
        }
        let sign = delta[axis].signum();
        let enter = (-sign * half[axis] - local[axis]) / delta[axis];
        let leave = (sign * half[axis] - local[axis]) / delta[axis];
        if enter > entry {
            entry = enter;
            normal = Vec3::ZERO;
            normal[axis] = -sign;
        }
        exit = exit.min(leave);
        if entry > exit {
            return None;
        }
    }
    (entry >= 0.0).then_some((entry, normal))
}

fn casing_contact(
    from: Vec3,
    to: Vec3,
    surfaces: &CasingSurfaces,
    removed: &map::DestroyedCells,
) -> Option<(Vec3, Vec3)> {
    let delta = to - from;
    if delta == Vec3::ZERO {
        return None;
    }
    map::BLOCKS
        .iter()
        .enumerate()
        .filter(|(i, _)| !map::destructible(*i))
        .map(|(_, block)| (block.center, block.half_size))
        .chain(surfaces.fixtures.iter().map(|b| (b.center, b.half_size)))
        .chain(surfaces.cells.iter().filter_map(|cell| {
            let id = usize::from(cell.id);
            (removed[id / 8] & (1 << (id % 8)) == 0).then_some((cell.center, cell.half_size))
        }))
        .filter_map(|(center, half)| {
            casing_box_entry(from, delta, Vec3::from(center), Vec3::from(half))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(fraction, normal)| {
            (
                from + delta * fraction + normal * CASING_SURFACE_CLEARANCE,
                normal,
            )
        })
}

fn step_casing(
    casing: &mut EjectedCasing,
    transform: &mut Transform,
    delta: f32,
    surfaces: &CasingSurfaces,
    removed: &map::DestroyedCells,
) -> bool {
    casing.lifetime -= delta;
    if casing.lifetime <= 0.0 {
        return true;
    }
    casing.velocity.y -= 9.81 * delta;
    let from = transform.translation;
    transform.translation += casing.velocity * delta;
    transform.rotation = Quat::from_euler(
        EulerRot::XYZ,
        casing.spin.x * delta,
        casing.spin.y * delta,
        casing.spin.z * delta,
    ) * transform.rotation;
    if let Some((point, normal)) = casing_contact(from, transform.translation, surfaces, removed) {
        transform.translation = point;
        let approach = casing.velocity.dot(normal);
        let tangent = casing.velocity - normal * approach;
        let rebound = if approach < -0.25 {
            -approach * 0.24
        } else {
            0.0
        };
        casing.velocity = tangent * 0.72 + normal * rebound;
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
    cover_and_surfaces: (
        Option<Res<crate::sync::cover::State>>,
        Local<Option<CasingSurfaces>>,
    ),
) {
    if state.raid != Some(session.raid.raid_id) {
        for entity in state.active.drain(..) {
            commands.entity(entity).despawn();
        }
        state.shots_by_shooter.clear();
        state.raid = Some(session.raid.raid_id);
    }

    let delta = time.delta_secs().min(0.05);
    let (cover, mut surfaces) = cover_and_surfaces;
    let surfaces = surfaces.get_or_insert_with(CasingSurfaces::new);
    let mut removed = map::destroyed_mask(&session.destroyed_cells);
    if let Some(cover) = cover
        .as_ref()
        .and_then(|state| state.current(&session.raid))
    {
        for (mask, reliable) in removed.iter_mut().zip(cover.removed) {
            *mask |= reliable;
        }
    }
    let mut expired = Vec::new();
    for (entity, mut casing, mut transform) in &mut casings {
        if step_casing(&mut casing, &mut transform, delta, surfaces, &removed) {
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
        let jitter = casing_jitter(shot.id as u32);
        let position = shot.ejection.map_or_else(
            || muzzle - direction * spec.ejection_back + right * spec.ejection_side + up * 0.035,
            Vec3::from,
        );
        if !position.is_finite() {
            continue;
        }
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
        if let Some(oldest) = state.track(entity) {
            commands.entity(oldest).despawn();
        }
    }
}
#[derive(Default)]
pub(super) struct Response {
    pub recoil: f32,
    velocity: f32,
    angular: recoil::Presentation,
    pub flash: f32,
    last: Option<u64>,
    weapon: Option<WeaponKind>,
    pub(super) displayed: Option<WeaponKind>,
    pub(super) action: Option<WeaponAction>,
    /// Mechanical state of the displayed slot, including unequip before the
    /// authoritative handoff and equip after it.
    pub(super) displayed_mechanics: common::catalog::MechanicalSnapshot,
    sway: Vec2,
    previous_look: Option<(Vec2, f32)>,
    look_rate: Vec2,
    travel: Vec3,
    inertia: Vec3,
    landing: f32,
    pub(super) ads: f32,
    was_alive: bool,
    frame_offset: Vec3,
}
impl Response {
    fn step(&mut self, dt: f32) {
        self.velocity += (-self.recoil * 180.0 - self.velocity * 23.0) * dt;
        self.recoil = (self.recoil + self.velocity * dt).clamp(0.0, 0.11);
        self.flash = (self.flash - dt).max(0.0);
    }
    fn shot(&mut self, shot: &FiredProjectileData) {
        if self.last.is_some_and(|id| id >= shot.id) {
            return;
        }
        self.last = Some(shot.id);
        self.flash = 0.055;
        let weapon = shot.weapon.class();
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
    presentation_time: f64,
}

impl Feedback {
    pub(super) fn camera(&self, id: u64, voluntary: CameraInput) -> CameraInput {
        let angles = self.responses.get(&id).map_or(Vec2::ZERO, |response| {
            response.angular.angles(self.presentation_time)
        });
        common::recoil::RecoilState {
            angles: angles.into(),
            ..default()
        }
        .camera(voluntary)
    }
}
#[derive(Component)]
pub(crate) struct Flash;
#[derive(Component)]
#[cfg(test)]
pub(super) struct Bolt;
#[derive(Component)]
#[cfg(test)]
pub(super) struct Magazine;
#[derive(Component)]
#[cfg(test)]
pub(super) struct SupportHand;
#[derive(Component)]
#[cfg(test)]
pub(super) struct Control;
#[derive(Component)]
#[cfg(test)]
pub(super) struct Forearm {
    pub support: bool,
}
#[derive(Component)]
#[cfg(test)]
pub(super) struct UpperArm {
    pub support: bool,
}

#[cfg(test)]
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

#[cfg(test)]
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

fn idle_breath(elapsed: f32, speed: f32, ads: f32, ready: bool) -> Vec3 {
    if !ready {
        return Vec3::ZERO;
    }
    let breath = (elapsed * (0.27 * std::f32::consts::TAU)).sin()
        * (1.0 - common::weapon_animation::ease(speed / common::PLAYER_WALK_SPEED))
        * (1.0 - ads);
    Vec3::Y * (0.002 * breath)
}

pub(super) fn advance(
    time: Res<Time>,
    real_time: Option<Res<Time<Real>>>,
    session: Res<ClientSession>,
    mut feedback: ResMut<Feedback>,
    local_input: Option<Res<LocalInput>>,
    local_id: Option<Res<PlayerId>>,
) {
    let now = real_time
        .as_ref()
        .map_or(time.elapsed_secs_f64(), |t| t.elapsed_secs_f64());
    feedback.presentation_time = now;
    if feedback.raid != session.raid.raid_id {
        feedback.responses.clear();
        feedback.raid = session.raid.raid_id;
    }
    for response in feedback.responses.values_mut() {
        response.step(time.delta_secs().min(1.0 / 30.0));
    }
    for player in &session.players {
        let response = feedback.responses.entry(player.id).or_default();
        let rate = session
            .configurations
            .cache
            .get(session.configurations.cache.epoch(), player.configuration)
            .map_or(player.weapon_id.spec().recoil.return_rate, |entry| {
                entry.resolved.recoil.return_rate
            });
        response.angular.observe(
            player.recoil.unpack(),
            session.last_sequence,
            now,
            (now - session.last_received).max(0.0) as f32,
            rate,
        );
        let changed_weapon = response.weapon != Some(player.weapon);
        if changed_weapon {
            response.weapon = Some(player.weapon);
            response.flash = 0.0;
        }
        // The server changes slots at the end of unequip. Delaying that marker
        // with another client timer borrows the wrong instance/mechanical state.
        response.displayed = Some(player.weapon);
        if player.alive && !response.was_alive {
            response.action = None;
        }
        response.was_alive = player.alive;
        if !player.alive {
            response.flash = 0.0;
            response.recoil = 0.0;
            response.velocity = 0.0;
            response.angular = Default::default();
        }
        let w = response.displayed.unwrap_or(player.weapon);
        response.displayed_mechanics = player.mechanics;
        let mechanics = response.displayed_mechanics;
        let target = common::weapon_animation::manipulation::action(w, &mechanics);
        if changed_weapon {
            // At handoff the incoming assembly begins in its own lowered pose,
            // never in the outgoing weapon's partially completed reload pose.
            response.action = Some(target);
        }
        let alpha = 1.0 - (-time.delta_secs().min(0.05) * 32.0).exp();
        let action = response.action.get_or_insert(target);
        action.root.translation += ((target.root.translation - action.root.translation) * alpha)
            .clamp_length_max(1.2 * time.delta_secs().min(0.05));
        let angle = action.root.rotation.angle_between(target.root.rotation);
        let rotation_alpha = alpha.min(5.0 * time.delta_secs().min(0.05) / angle.max(1e-6));
        action.root.rotation = action
            .root
            .rotation
            .slerp(target.root.rotation, rotation_alpha);
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
        action.magazine_grip += (target.magazine_grip - action.magazine_grip) * alpha;
        action.control_grip += (target.control_grip - action.control_grip) * alpha;
        action.bolt += (target.bolt - action.bolt) * alpha;
        action.control += (target.control - action.control) * alpha;
        action.latch += (target.latch - action.latch) * alpha;
        if w == WeaponKind::Rifle {
            use common::catalog::ManipulationPhase;
            // The authored seat dwell lets the filter settle before insertion;
            // a committed mechanical location must never lag behind on screen.
            // A cancellation snapshot may be the first one showing the seat.
            if mechanics.magazine_inserted
                && (!mechanics.reloading()
                    || matches!(
                        mechanics.phase,
                        Some(ManipulationPhase::Feed | ManipulationPhase::Finish)
                    ))
            {
                action.magazine = target.magazine;
            }
            if mechanics.reloading() {
                action.bolt = if mechanics.bolt_open { 1.0 } else { 0.0 };
            }
        }
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
        let inertia = (Vec3::from(player.pose.velocity) - response.travel)
            * Vec3::new(-0.003, -0.002, -0.004);
        response.inertia = response
            .inertia
            .lerp(inertia, 1.0 - (-time.delta_secs() * 10.0).exp());
        let speed = Vec2::new(response.travel.x, response.travel.z).length();
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
            && player.mechanics.kind.is_none()
            && player.mechanics.equipped
            && !common::weapon_animation::manipulation::lowering(&mechanics)
            && player
                .body
                .anatomy
                .capacity(common::combat::Region::RightArm)
                > 0.0
            && player.pose.vault == 0
            && !(player.pose.sprinting && speed > 0.5);
        // Complete the external action's return before closing the stock onto
        // the sight line. Blending full ADS over a still-canted inspection pulls
        // the hands across the camera even when each individual layer is smooth.
        let action_return = response.action.map_or(1.0, |a| {
            common::weapon_animation::ease(
                1.0 - (a.root.rotation.angle_between(Quat::IDENTITY) / 0.35).min(1.0),
            )
        });
        let target = if usable {
            requested * action_return
        } else {
            0.0
        };
        let aim_step = (target - response.ads) * (1.0 - (-time.delta_secs() * 14.0).exp());
        response.ads += aim_step.clamp(-2.0 * time.delta_secs(), 2.0 * time.delta_secs());
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

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct LocalAnimationState<'w> {
    time: Res<'w, Time>,
    real_time: Option<Res<'w, Time<Real>>>,
    id: Res<'w, PlayerId>,
    session: Res<'w, ClientSession>,
    feedback: Res<'w, Feedback>,
    input: Res<'w, LocalInput>,
}

pub(super) fn animate_local(
    state: LocalAnimationState,
    mut views: Query<(&WeaponViewModel, &mut Transform), With<LocalView>>,
    #[cfg(test)] mut parts: Query<
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
    #[cfg(test)] roots: Query<&ChildOf, With<WeaponMeshRoot>>,
    #[cfg(test)] weapons: Query<&WeaponViewModel>,
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
        real_time,
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
    let mut stride = common::humanoid::PoseState {
        phase: p.pose.phase,
        velocity: response.travel.into(),
        acceleration: p.pose.acceleration,
        turn_rate: p.pose.turn_rate,
        mode: p.pose.mode,
        prone: p.pose.prone,
        sprinting: p.pose.sprinting,
        vault: p.pose.vault,
        ads: response.ads,
        ..default()
    };
    if let Some(motion) =
        real_time.and_then(|time| session.local_motion.sample(time.elapsed_secs_f64()))
    {
        stride.phase = motion.stride_phase;
        stride.velocity = motion.velocity.into();
        stride.acceleration = motion.acceleration.into();
        stride.turn_rate = motion.turn_rate;
        stride.set_vault(motion.vault);
    }
    let speed = stride.velocity[0].hypot(stride.velocity[2]);
    let phase = stride.phase;
    let motion_weight = 1.0 - response.ads * 0.92;
    let displayed = response.displayed.unwrap_or(p.weapon);
    let action = response
        .action
        .unwrap_or_else(|| reload_action(displayed, 0.0, false));
    let spec = session
        .configurations
        .cache
        .get(session.configurations.cache.epoch(), p.configuration)
        .map_or_else(|| displayed.spec(), |entry| &entry.resolved);
    use common::catalog::AnchorKind;
    let carry = spec
        .assembly
        .anchors
        .get(&AnchorKind::SupportGrip)
        .filter(|_| p.alive && p.pose.physical.is_none())
        .map_or_else(common::humanoid::armed_stride::Carry::default, |support| {
            common::humanoid::armed_stride::sample(&stride, *support, action)
        });
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
        t.translation += locomotion_offset(phase, speed, response.ads) * (1.0 - carry.weight)
            + carry.translation
            + idle_breath(
                time.elapsed_secs(),
                speed,
                response.ads,
                p.alive && p.pose.mode == PhysicalMode::Standing,
            )
            + action.root.translation
            + Vec3::new(
                response.sway.x * 0.2 * (1.0 - carry.weight),
                0.0,
                response.recoil,
            );
        t.translation += response.inertia * (1.0 - carry.weight);
        let vault = common::humanoid::vault_weight(stride.vault);
        t.translation += Vec3::new(0.0, -0.10, 0.06) * vault;
        t.rotation *= Quat::from_rotation_x(0.25 * vault);
        t.translation.y -= response.landing * 0.025 * motion_weight;
        t.rotation *=
            Quat::from_rotation_x((response.travel.y * 0.008).clamp(-0.05, 0.05) * motion_weight);
        let injury = (1.0 - p.body.anatomy.handling()) * (time.elapsed_secs() * 3.7).sin() * 0.035;
        let sway = response.sway * (1.0 - carry.weight);
        t.rotation *= Quat::from_rotation_x(sway.y * motion_weight);
        t.rotation *= Quat::from_rotation_y(sway.x * motion_weight);
        t.rotation *= Quat::from_rotation_z((injury - sway.x * 0.5) * motion_weight);
        {
            let aspect = if let Projection::Perspective(p) = &**projection {
                p.aspect_ratio
            } else {
                1.0
            };
            let mut offset = super::weapons::framed_offset(displayed, aspect).lerp(
                common::weapon_animation::ads_offset(displayed),
                common::weapon_animation::ease(response.ads),
            );
            if displayed != WeaponKind::Pistol {
                offset = spec.assembly.hip_from_root.translation.lerp(
                    spec.assembly.ads_from_root.translation,
                    common::weapon_animation::ease(response.ads),
                );
            }
            // The view rig starts at the eye. Apply the same trigger-pivoted
            // carry as the world rig, without forwarding it to either camera.
            let carry_rotation = carry.rotation();
            let pivot = offset
                + spec
                    .assembly
                    .anchors
                    .get(&AnchorKind::TriggerGrip)
                    .map_or(Vec3::ZERO, |anchor| anchor.translation);
            let carry_correction = t.rotation * (pivot - carry_rotation * pivot);
            t.rotation *= carry_rotation;
            // Cant about the receiver, not the eye. The complete configured
            // hierarchy follows this rigid layer, including both grip contacts.
            t.translation = base.translation
                + base.rotation * (t.translation - base.translation)
                + carry_correction
                + t.rotation * (offset - action.root.rotation * offset);
        }
        t.rotation *= action.root.rotation;
        t.translation.y -= physical_drop * 0.34;
        t.rotation *= Quat::from_rotation_x(physical_drop * 0.6);
    }
    // Retained only as a migration oracle for the pre-configured regression
    // fixtures. Production parts and arms are exclusively configured.
    #[cfg(test)]
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
                p.mechanics.reloading(),
            );
            if displayed == WeaponKind::Pistol && p.mechanics.bolt_open && !p.mechanics.reloading()
            {
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
            *visibility = if action.magazine_visible
                && (p.mechanics.magazine_inserted || p.mechanics.reloading())
            {
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
            let to_local = Quat::from_rotation_y(p.rot.yaw - input.camera.yaw);
            t.translation = super::humanoid_eye(
                to_local * Vec3::from(head.position),
                to_local * Quat::from_array(head.rotation),
            );
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
#[cfg(test)]
pub(super) struct ViewFrameBounds(pub [Vec3; 8]);

#[derive(bevy::ecs::system::SystemParam)]
struct LocalFramingState<'w> {
    time: Res<'w, Time>,
    id: Res<'w, PlayerId>,
    session: Res<'w, ClientSession>,
    feedback: ResMut<'w, Feedback>,
}

#[cfg(test)]
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
        let lowering =
            common::weapon_animation::manipulation::lowering(&response.displayed_mechanics)
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
    #[test]
    fn idle_breathing_is_subtle_and_quiet_during_motion_or_ads() {
        use super::*;
        let idle = idle_breath(1.0 / (4.0 * 0.27), 0.0, 0.0, true);
        assert!((idle.y - 0.002).abs() < 1e-6);
        assert_eq!(idle_breath(1.0, PLAYER_WALK_SPEED, 0.0, true), Vec3::ZERO);
        assert_eq!(idle_breath(1.0, 0.0, 1.0, true), Vec3::ZERO);
        assert_eq!(idle_breath(1.0, 0.0, 0.0, false), Vec3::ZERO);
    }

    fn reload_snapshot(
        weapon: common::WeaponKind,
        fraction: f32,
        empty: bool,
    ) -> common::catalog::MechanicalSnapshot {
        use common::catalog::*;
        let mut state = MechanicalState::new(
            catalog(),
            weapon.spec(),
            [1, 2, 3, 4].map(MagazineInstanceId),
        )
        .unwrap();
        for _ in 0..if empty {
            weapon.spec().magazine_size
        } else {
            5
        } {
            assert!(state.fire(true, true));
        }
        assert!(state.start_reload(10.0));
        state.advance(fraction * 10.0);
        state.snapshot()
    }
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
    fn shell_casing_tracking_evicts_oldest_and_lifetime_expires() {
        let mut world = World::new();
        let mut state = CasingState::default();
        let entities: Vec<_> = (0..MAX_ACTIVE_CASINGS * 2)
            .map(|_| world.spawn_empty().id())
            .collect();
        for (i, &entity) in entities.iter().enumerate() {
            assert_eq!(
                state.track(entity),
                i.checked_sub(MAX_ACTIVE_CASINGS).map(|old| entities[old])
            );
            assert!(state.active.len() <= MAX_ACTIVE_CASINGS);
        }
        assert!(
            state
                .active
                .into_iter()
                .eq(entities[MAX_ACTIVE_CASINGS..].iter().copied())
        );

        let mut casing = EjectedCasing {
            velocity: Vec3::ZERO,
            spin: Vec3::ZERO,
            lifetime: CASING_LIFETIME,
        };
        let mut transform = Transform::from_xyz(0.0, 1.0, 0.0);
        let surfaces = CasingSurfaces {
            cells: vec![],
            fixtures: vec![],
        };
        let removed = map::destroyed_mask(&[]);
        assert!(!step_casing(
            &mut casing,
            &mut transform,
            CASING_LIFETIME - 0.1,
            &surfaces,
            &removed,
        ));
        assert!((casing.lifetime - 0.1).abs() < 0.001);
        assert!(step_casing(
            &mut casing,
            &mut transform,
            0.2,
            &surfaces,
            &removed
        ));
    }

    #[test]
    fn shell_casing_lands_on_crossed_surface_not_endpoint_floor() {
        let mut surfaces = CasingSurfaces {
            cells: map::barrier_cells(),
            fixtures: vec![],
        };
        let intact = map::destroyed_mask(&[]);
        let from = Vec3::new(0.0, 1.4, -3.0);
        let to = Vec3::new(0.0, 0.0, -1.0);
        let (cover, normal) = casing_contact(from, to, &surfaces, &intact).unwrap();
        assert_eq!(normal, Vec3::Y);
        assert!((cover.y - 1.205).abs() < 1e-5);
        assert!(cover.z < -2.4);
        let removed = map::destroyed_mask(
            &surfaces
                .cells
                .iter()
                .map(|cell| cell.id)
                .collect::<Vec<_>>(),
        );
        let (floor, normal) = casing_contact(from, to, &surfaces, &removed).unwrap();
        assert_eq!(normal, Vec3::Y);
        assert!((floor.y - 0.105).abs() < 1e-5);
        let from = Vec3::new(-1.55, 0.5, -1.75);
        let to = Vec3::new(-1.55, 0.0, -1.75);
        assert!((casing_contact(from, to, &surfaces, &intact).unwrap().0.y - 0.105).abs() < 1e-5);
        surfaces.fixtures.push(map::POV_STEP);
        assert!((casing_contact(from, to, &surfaces, &intact).unwrap().0.y - 0.285).abs() < 1e-5);
    }

    #[test]
    fn shell_casing_rebounds_off_intact_side_and_passes_removed_cell() {
        let surfaces = CasingSurfaces {
            cells: map::barrier_cells(),
            fixtures: vec![],
        };
        let start = Vec3::new(0.1, 0.9, -2.2);
        let run = |removed: &map::DestroyedCells| {
            let mut casing = EjectedCasing {
                velocity: Vec3::NEG_Z * 10.0,
                spin: Vec3::ZERO,
                lifetime: CASING_LIFETIME,
            };
            let mut transform = Transform::from_translation(start);
            assert!(!step_casing(
                &mut casing,
                &mut transform,
                0.05,
                &surfaces,
                removed
            ));
            (transform.translation, casing.velocity)
        };
        let (point, velocity) = run(&map::destroyed_mask(&[]));
        assert!((point.z + 2.395).abs() < 1e-5);
        assert!((velocity.z - 2.4).abs() < 1e-5);
        let cell = surfaces
            .cells
            .iter()
            .find(|cell| {
                let center = Vec3::from(cell.center);
                let half = Vec3::from(cell.half_size);
                let contact = point - Vec3::Z * CASING_SURFACE_CLEARANCE;
                ((center - contact).abs() - half).max_element() < 1e-5
            })
            .unwrap();
        let (point, velocity) = run(&map::destroyed_mask(&[cell.id]));
        assert!((point.z + 2.7).abs() < 1e-5);
        assert_eq!(velocity.z, -10.0);
    }

    #[test]
    fn shell_casing_dedup_tracks_each_shooter_independently() {
        let mut state = CasingState::default();
        assert!(state.should_eject(7, 10));
        assert!(state.should_eject(8, 9));
        assert!(!state.should_eject(7, 10));
        assert!(
            state.should_eject(8, 8),
            "delayed reliable event still ejects once"
        );
        assert!(!state.should_eject(8, 8));
    }

    #[test]
    fn confirmed_event_has_cosmetic_kick_but_does_not_inject_angular_recoil() {
        let mut response = Response::default();
        response.shot(&FiredProjectileData {
            id: 1,
            weapon: WeaponId::M4A1,
            shooter: 7,
            time: 0.0,
            muzzle: [0.0; 3],
            direction: [0.0, 0.0, -1.0],
            ejection: None,
            audio_signature: 100,
        });
        assert!((response.recoil - 0.010).abs() < f32::EPSILON);
        assert_eq!(response.angular.angles(0.1), Vec2::ZERO);
    }

    #[test]
    fn local_camera_recoil_never_changes_serialized_voluntary_input() {
        let mut app = App::new();
        let mut feedback = Feedback::default();
        let response = feedback.responses.entry(7).or_default();
        response.angular.observe(
            common::recoil::RecoilState {
                angles: [0.02, 0.08],
                ..default()
            },
            1,
            0.0,
            0.0,
            14.0,
        );
        feedback.presentation_time = 0.05;
        app.insert_resource(feedback)
            .insert_resource(PlayerId(7))
            .init_resource::<LocalInput>()
            .add_systems(
                Update,
                (
                    super::super::sync_local_view,
                    super::super::sync_local_player_rotation,
                ),
            );
        app.world_mut().spawn((
            LocalPlayer,
            PlayerVisualState::default(),
            Transform::default(),
        ));
        let view = app
            .world_mut()
            .spawn((
                LocalView,
                super::super::BaseLocalOffset(Vec3::ZERO),
                Transform::default(),
            ))
            .id();
        let before = data::encode(&**app.world().resource::<LocalInput>());
        app.update();
        let bore = app.world().get::<Transform>(view).unwrap().rotation * Vec3::NEG_Z;
        assert!(bore.y > 0.0, "presentation must raise the view: {bore:?}");
        assert_eq!(
            data::encode(&**app.world().resource::<LocalInput>()),
            before
        );
        app.world_mut().resource_mut::<Feedback>().presentation_time = 2.0;
        app.update();
        let bore = app.world().get::<Transform>(view).unwrap().rotation * Vec3::NEG_Z;
        assert!(bore.y.abs() < 0.0001);
        assert_eq!(
            data::encode(&**app.world().resource::<LocalInput>()),
            before
        );
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
                        session.players[0].mechanics = if i < 180 {
                            reload_snapshot(weapon, fraction, empty)
                        } else {
                            default()
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
                        mechanics: if fraction > 0.0 {
                            reload_snapshot(weapon, fraction, true)
                        } else {
                            default()
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
    fn authoritative_slot_marker_reveals_lowered_model_and_reload_interrupt_blends_from_current_pose()
     {
        let mut app = App::new();
        let mut session = ClientSession::default();
        session.players.push(ClientData {
            id: 7,
            alive: true,
            weapon: WeaponKind::Pistol,
            weapon_id: WeaponId::M1911,
            weapon_slot: EquipmentSlot::Sidearm,
            ammo_in_mag: 12,
            mechanics: reload_snapshot(WeaponKind::Pistol, 0.5, false),
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
        app.world_mut().resource_mut::<ClientSession>().players[0].mechanics = default();
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
        app.world_mut().resource_mut::<ClientSession>().players[0]
            .mechanics
            .kind = Some(common::catalog::ManipulationKind::Unequip);
        app.world_mut().resource_mut::<ClientSession>().players[0]
            .mechanics
            .fraction = 0.8;
        for _ in 0..60 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<Feedback>().responses[&7].displayed,
            Some(WeaponKind::Pistol),
            "elapsed client time cannot advance the authoritative slot marker"
        );
        app.world_mut().resource_mut::<ClientSession>().players[0].weapon = WeaponKind::Rifle;
        app.world_mut().resource_mut::<ClientSession>().players[0].weapon_id = WeaponId::M4A1;
        app.world_mut().resource_mut::<ClientSession>().players[0]
            .mechanics
            .kind = Some(common::catalog::ManipulationKind::Equip);
        app.world_mut().resource_mut::<ClientSession>().players[0]
            .mechanics
            .fraction = 0.0;
        app.update();
        assert_eq!(
            app.world().resource::<Feedback>().responses[&7].displayed,
            Some(WeaponKind::Rifle)
        );
        assert!(
            app.world().resource::<Feedback>().responses[&7]
                .action
                .unwrap()
                .root
                .translation
                .y
                < -0.33
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
            ejection: None,
            audio_signature: 100,
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
            ejection: None,
            audio_signature: 100,
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
