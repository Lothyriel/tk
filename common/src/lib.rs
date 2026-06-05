pub mod data;

use bevy::{platform::collections::HashMap, prelude::*};
use bevy_rapier3d::prelude::*;
use bevy_renet2::{netcode::NetcodeTransportError, prelude::ClientId};
use rkyv::{Archive, Deserialize, Serialize};

pub const DEFAULT_PORT: u16 = 9080;
pub const PROTOCOL_ID: u64 = 0;
pub const PLAYER_COLLIDER_RADIUS: f32 = 0.35;
pub const PLAYER_COLLIDER_HALF_HEIGHT: f32 = 0.55;
pub const PLAYER_STEP_HEIGHT: f32 = 0.25;
pub const PLAYER_WALK_SPEED: f32 = 3.25;
pub const PLAYER_RUN_SPEED: f32 = 5.5;
pub const PLAYER_GROUND_ACCELERATION: f32 = 10.0;
pub const PLAYER_GROUND_DECELERATION: f32 = 7.5;
pub const PLAYER_AIR_ACCELERATION: f32 = 4.0;
pub const PLAYER_AIR_CONTROL: f32 = 0.2;
pub const PLAYER_GRAVITY: f32 = 20.0;
pub const PLAYER_JUMP_SPEED: f32 = 6.5;

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
}

#[derive(Debug, Default, Archive, Serialize, Deserialize, Component, Resource)]
pub struct ClientInput {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub run: bool,
    pub jump: bool,
    pub camera: CameraInput,
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
