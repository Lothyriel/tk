pub mod catalog;
pub mod combat;
pub mod data;
pub mod humanoid;
pub mod map;
pub mod raid;
pub mod recoil;
pub mod replication;
pub mod weapon_animation;

use bevy::{platform::collections::HashMap, prelude::*};
use bevy_rapier3d::prelude::*;
use bevy_renet2::{netcode::NetcodeTransportError, prelude::ClientId};
pub use catalog::{EquipmentSlot, WeaponId, WeaponInstanceId};
use rkyv::{Archive, Deserialize, Serialize};

pub const DEFAULT_PORT: u16 = 9080;
pub const PROTOCOL_ID: u64 = 17;
pub const SNAPSHOT_INTERVAL: f32 = 1.0 / 60.0;
pub const PLAYER_COLLIDER_RADIUS: f32 = 0.35;
pub const PLAYER_COLLIDER_HALF_HEIGHT: f32 = 0.55;
pub const PLAYER_CROUCH_COLLIDER_HALF_HEIGHT: f32 = 0.25;
pub const PLAYER_STEP_HEIGHT: f32 = 0.25;
pub const PLAYER_CROUCH_SCALE: f32 = (PLAYER_CROUCH_COLLIDER_HALF_HEIGHT + PLAYER_COLLIDER_RADIUS)
    / (PLAYER_COLLIDER_HALF_HEIGHT + PLAYER_COLLIDER_RADIUS);
pub const PLAYER_CROUCH_VIEW_OFFSET: f32 = -0.18;
pub const PLAYER_VIEW_HEIGHT: f32 = 0.55;
pub const PLAYER_WALK_SPEED: f32 = 3.25;
pub const PLAYER_RUN_SPEED: f32 = 5.5;
pub const PLAYER_CROUCH_SPEED: f32 = 1.75;
pub const PLAYER_GROUND_ACCELERATION: f32 = 10.0;
pub const PLAYER_GROUND_DECELERATION: f32 = 7.5;
pub const PLAYER_AIR_ACCELERATION: f32 = 4.0;
pub const PLAYER_AIR_CONTROL: f32 = 0.2;
pub const PLAYER_GRAVITY: f32 = 20.0;
pub const PLAYER_JUMP_SPEED: f32 = 6.5;
pub const PLAYER_MAX_HEALTH: f32 = 250.0;
pub const PROJECTILE_LIFETIME: f32 = 3.0;
pub const PROJECTILE_GRAVITY: f32 = 9.81;
pub const PLAYER_RESPAWN_HEIGHT: f32 = 1.5;

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        catalog::catalog();
        app.init_resource::<Lobby>()
            .add_plugins(RapierPhysicsPlugin::<NoUserData>::default().in_fixed_schedule())
            .add_systems(Update, report_transport_errors);
    }
}

fn report_transport_errors(mut renet_error: MessageReader<NetcodeTransportError>) {
    for error in renet_error.read() {
        warn!("Network transport: {error}");
    }
}

#[derive(Debug, Archive, Serialize, Deserialize, Resource)]
pub struct PlayerId(pub u64);

#[derive(Debug, Component)]
pub struct LocalPlayer;

#[derive(Debug, Default, Resource, Deref, DerefMut)]
pub struct LocalInput(pub ClientInput);

#[derive(Debug, Default, Clone, Archive, Serialize, Deserialize)]
pub struct ClientData {
    // Group scalar IDs and flags to avoid per-actor archive padding. The compact
    // configuration reference must fit the existing physical snapshot budget.
    pub id: ClientId,
    pub weapon_instance: WeaponInstanceId,
    pub pos: [f32; 3],
    pub rot: CameraInput,
    pub health: f32,
    pub weapon_id: WeaponId,
    pub configuration: u32,
    pub ammo_in_mag: u32,
    pub reserve_ammo: u32,
    pub weapon: WeaponKind,
    pub weapon_slot: EquipmentSlot,
    pub crouched: bool,
    pub alive: bool,
    pub mechanics: catalog::MechanicalSnapshot,
    pub recoil: recoil::PackedRecoil,
    pub body: combat::BodyHealth,
    pub armor: combat::ArmorState,
    pub participant: raid::Participant,
    pub pose: humanoid::PoseState,
    pub bot: bool,
}

impl ClientData {
    pub fn reload_remaining(&self) -> f32 {
        if self.mechanics.reloading() {
            self.mechanics.remaining
        } else {
            0.0
        }
    }
}

#[derive(Debug, Default, Archive, Serialize, Deserialize, Component)]
pub struct ClientInput {
    pub lobby_sequence: u32,
    pub loadout: combat::Loadout,
    pub interact: bool,
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub run: bool,
    pub crouch: bool,
    pub jump: bool,
    pub fire: bool,
    pub aim: bool,
    pub fire_pressed_sequence: u32,
    pub reload_sequence: u32,
    pub inspect_sequence: u32,
    pub selector_sequence: u32,
    pub weapon: EquipmentSlot,
    pub camera: CameraInput,
}

impl ClientInput {
    pub fn weapon_id(&self) -> WeaponId {
        self.loadout.weapon(self.weapon)
    }
}

/// Presentation/mechanism family, not a catalog identity or equipment slot.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize, serde::Deserialize,
)]
pub enum WeaponKind {
    #[default]
    Rifle,
    Pistol,
    Smg,
}

impl WeaponKind {
    pub fn index(self) -> usize {
        match self {
            Self::Rifle => 0,
            Self::Pistol => 1,
            Self::Smg => 2,
        }
    }

    /// Transitional base selection for legacy callers during the ID migration.
    pub fn definition(self) -> catalog::WeaponId {
        match self {
            Self::Rifle => catalog::WeaponId::M4A1,
            Self::Pistol => catalog::WeaponId::M1911,
            Self::Smg => catalog::WeaponId::MP5,
        }
    }

    pub fn spec(self) -> &'static catalog::ResolvedWeapon {
        self.definition().spec()
    }
}

#[derive(Debug, Default, Clone, Copy, Archive, Serialize, Deserialize, Component)]
pub struct CameraInput {
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
}

const DEFAULT_EULER_ROT: EulerRot = EulerRot::YXZ;

impl From<&CameraInput> for Quat {
    fn from(val: &CameraInput) -> Self {
        let CameraInput { pitch, yaw, roll } = val;
        Quat::from_euler(DEFAULT_EULER_ROT, *yaw, *pitch, *roll)
    }
}

impl From<Quat> for CameraInput {
    fn from(value: Quat) -> Self {
        let (yaw, pitch, roll) = value.to_euler(DEFAULT_EULER_ROT);

        Self { yaw, pitch, roll }
    }
}

#[derive(Debug, Component)]
pub struct Client {
    pub id: ClientId,
}

#[derive(Debug, Default, Component)]
pub struct MovementState {
    pub velocity: Vec3,
    pub grounded: bool,
    pub jump_queued: bool,
    pub crouched: bool,
}

#[derive(Debug, Default, Component)]
pub struct PlayerVisualState {
    pub alive: bool,
    pub crouched: bool,
    pub health: f32,
    pub weapon: WeaponKind,
    pub ammo_in_mag: u32,
}

#[derive(Debug, Archive, Serialize, Deserialize)]
pub struct ProjectileData {
    pub id: u64,
    pub pos: [f32; 3],
    pub vel: [f32; 3],
}

#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
pub struct FiredProjectileData {
    // Keep the two 64-bit fields together to avoid padding per retained shot.
    pub id: u64,
    pub shooter: u64,
    pub weapon: WeaponId,
    pub time: f32,
    pub muzzle: [f32; 3],
    pub direction: [f32; 3],
    /// Cosmetic casing origin sampled at the shot, if the assembly defines one.
    pub ejection: Option<[f32; 3]>,
    /// Game-audio signature from the assembly at the instant of the shot.
    pub audio_signature: u8,
}

impl FiredProjectileData {
    pub fn encode_audio_signature(multiplier: f32) -> u8 {
        (multiplier * 100.0).round().clamp(1.0, 255.0) as u8
    }
}

#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
pub struct PhysicsPieceData {
    pub id: u64,
    pub owner: u64,
    pub segment: humanoid::Segment,
    pub pose: humanoid::SegmentPose,
    pub detached: bool,
    pub bot: bool,
}

#[derive(Debug, Archive, Serialize, Deserialize, Clone)]
pub struct ImpactMarkData {
    pub id: u32,
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub size_mm: u8,
}

#[derive(Debug, Archive, Serialize, Deserialize)]
pub struct WorldSnapshot {
    pub destroyed_cells: map::DestroyedCells,
    pub sequence: u64,
    pub configuration_epoch: u64,
    pub raid: raid::RaidState,
    pub players: Vec<ClientData>,
    pub projectiles: Vec<ProjectileData>,
    pub impact_marks: Vec<ImpactMarkData>,
    pub fired_projectiles: Vec<FiredProjectileData>,
    pub physics_pieces: Vec<PhysicsPieceData>,
    pub simulation_time: f32,
}

impl WorldSnapshot {
    /// Validate content references before presentation can dereference IDs.
    pub fn catalog_valid(&self) -> bool {
        let catalog = catalog::catalog();
        self.players.iter().all(|p| {
            catalog
                .weapon(p.weapon_id)
                .is_some_and(|w| w.class == p.weapon)
                && p.participant.loadout.validated().is_some()
        }) && self
            .fired_projectiles
            .iter()
            .all(|s| catalog.weapon(s.weapon).is_some())
    }
}

#[derive(Debug, Default, Resource)]
pub struct Lobby {
    pub players: HashMap<ClientId, Entity>,
}

#[derive(Debug, Archive, Serialize, Deserialize, Component)]
pub enum ServerMessage {
    ClientConnected { id: ClientId },
    ClientDisconnected { id: ClientId },
}
