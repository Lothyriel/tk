pub mod data;

use bevy::{platform::collections::HashMap, prelude::*};
use bevy_rapier3d::prelude::*;
use bevy_renet2::{netcode::NetcodeTransportError, prelude::ClientId};
use serde::{Deserialize, Serialize};

pub const DEFAULT_PORT: u16 = 9080;
pub const PROTOCOL_ID: u64 = 0;
pub const PLAYER_MOVE_SPEED: f32 = 5.0;

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_resource::<Lobby>()
            .add_plugins(RapierPhysicsPlugin::<NoUserData>::default())
            .add_systems(Update, panic_on_error_system);
    }
}

fn panic_on_error_system(mut renet_error: EventReader<NetcodeTransportError>) {
    if let Some(e) = renet_error.read().next() {
        panic!("{}", e);
    }
}

#[derive(Debug, Serialize, Deserialize, Component, Resource)]
pub struct PlayerId(pub u64);

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientData {
    pub id: ClientId,
    pub pos: [f32; 3],
    pub rot: CameraInput,
}

#[derive(Debug, Default, Serialize, Deserialize, Component, Resource)]
pub struct ClientInput {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub camera: CameraInput,
}

#[derive(Debug, Default, Serialize, Deserialize, Component)]
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

#[derive(Debug, Default, Resource)]
pub struct Lobby {
    pub players: HashMap<ClientId, Entity>,
}

#[derive(Debug, Serialize, Deserialize, Component)]
pub enum ServerMessage {
    ClientConnected { id: ClientId },
    ClientDisconnected { id: ClientId },
}
