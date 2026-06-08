pub mod data;

use bevy::{platform::collections::HashMap, prelude::*};
use bevy_rapier3d::prelude::*;
use bevy_renet2::{netcode::NetcodeTransportError, prelude::ClientId};
use rkyv::{Archive, Deserialize, Serialize};

pub const DEFAULT_PORT: u16 = 9080;
pub const PROTOCOL_ID: u64 = 0;
pub const PLAYER_COLLIDER_RADIUS: f32 = 0.35;
pub const PLAYER_COLLIDER_HALF_HEIGHT: f32 = 0.55;
pub const PLAYER_CROUCH_COLLIDER_HALF_HEIGHT: f32 = 0.25;
pub const PLAYER_STEP_HEIGHT: f32 = 0.25;
pub const PLAYER_CROUCH_SCALE: f32 =
    (PLAYER_CROUCH_COLLIDER_HALF_HEIGHT + PLAYER_COLLIDER_RADIUS)
        / (PLAYER_COLLIDER_HALF_HEIGHT + PLAYER_COLLIDER_RADIUS);
pub const PLAYER_CROUCH_VIEW_OFFSET: f32 = -0.35;
pub const PLAYER_WALK_SPEED: f32 = 3.25;
pub const PLAYER_RUN_SPEED: f32 = 5.5;
pub const PLAYER_CROUCH_SPEED: f32 = 1.75;
pub const PLAYER_GROUND_ACCELERATION: f32 = 10.0;
pub const PLAYER_GROUND_DECELERATION: f32 = 7.5;
pub const PLAYER_AIR_ACCELERATION: f32 = 4.0;
pub const PLAYER_AIR_CONTROL: f32 = 0.2;
pub const PLAYER_GRAVITY: f32 = 20.0;
pub const PLAYER_JUMP_SPEED: f32 = 6.5;
pub const PLAYER_MAX_HEALTH: f32 = 100.0;
pub const PROJECTILE_LIFETIME: f32 = 3.0;
pub const PROJECTILE_GRAVITY: f32 = 9.81;
pub const PLAYER_RESPAWN_HEIGHT: f32 = 1.5;

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_resource::<Lobby>()
            .add_plugins(RapierPhysicsPlugin::<NoUserData>::default().in_fixed_schedule())
            .add_systems(Update, panic_on_error_system);
    }
}

fn panic_on_error_system(mut renet_error: MessageReader<NetcodeTransportError>) {
    if let Some(e) = renet_error.read().next() {
        panic!("{}", e);
    }
}

#[derive(Debug, Archive, Serialize, Deserialize, Component, Resource)]
pub struct PlayerId(pub u64);

#[derive(Debug, Archive, Serialize, Deserialize)]
pub struct ClientData {
    pub id: ClientId,
    pub pos: [f32; 3],
    pub rot: CameraInput,
    pub crouched: bool,
    pub alive: bool,
    pub health: f32,
    pub weapon: WeaponKind,
    pub ammo_in_mag: u32,
}

#[derive(Debug, Default, Archive, Serialize, Deserialize, Component, Resource)]
pub struct ClientInput {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub run: bool,
    pub crouch: bool,
    pub jump: bool,
    pub respawn_sequence: u32,
    pub fire: bool,
    pub fire_pressed_sequence: u32,
    pub reload_sequence: u32,
    pub weapon: WeaponKind,
    pub camera: CameraInput,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub enum WeaponKind {
    #[default]
    Rifle,
    Pistol,
}

impl WeaponKind {
    pub fn spec(self) -> WeaponSpec {
        match self {
            Self::Rifle => WeaponSpec {
                kind: self,
                name: "Rifle",
                magazine_size: 30,
                rounds_per_minute: 600.0,
                reload_seconds: 2.4,
                muzzle_speed: 715.0,
                damage: 34.0,
                automatic: true,
                model_scale: [0.18, 0.12, 0.9],
                model_offset: [0.28, -0.18, -0.45],
                model_color: [0.16, 0.16, 0.16],
                barrel_offset: [0.28, -0.15, -0.92],
            },
            Self::Pistol => WeaponSpec {
                kind: self,
                name: "Pistol",
                magazine_size: 17,
                rounds_per_minute: 400.0,
                reload_seconds: 1.5,
                muzzle_speed: 375.0,
                damage: 26.0,
                automatic: false,
                model_scale: [0.12, 0.1, 0.35],
                model_offset: [0.2, -0.18, -0.25],
                model_color: [0.22, 0.22, 0.24],
                barrel_offset: [0.2, -0.15, -0.72],
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WeaponSpec {
    pub kind: WeaponKind,
    pub name: &'static str,
    pub magazine_size: u32,
    pub rounds_per_minute: f32,
    pub reload_seconds: f32,
    pub muzzle_speed: f32,
    pub damage: f32,
    pub automatic: bool,
    pub model_scale: [f32; 3],
    pub model_offset: [f32; 3],
    pub model_color: [f32; 3],
    pub barrel_offset: [f32; 3],
}

impl WeaponSpec {
    pub fn seconds_per_shot(self) -> f32 {
        60.0 / self.rounds_per_minute
    }
}

#[derive(Debug, Default, Archive, Serialize, Deserialize, Component)]
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
    pub id: u64,
    pub weapon: WeaponKind,
}

#[derive(Debug, Archive, Serialize, Deserialize, Clone)]
pub struct ImpactMarkData {
    pub id: u64,
    pub pos: [f32; 3],
    pub normal: [f32; 3],
}

#[derive(Debug, Archive, Serialize, Deserialize)]
pub struct WorldSnapshot {
    pub players: Vec<ClientData>,
    pub projectiles: Vec<ProjectileData>,
    pub impact_marks: Vec<ImpactMarkData>,
    pub fired_projectiles: Vec<FiredProjectileData>,
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
