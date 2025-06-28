pub mod data;

use bevy::{platform::collections::HashMap, prelude::*};
use bevy_renet2::{netcode::NetcodeTransportError, prelude::ClientId};
use serde::{Deserialize, Serialize};

pub const DEFAULT_PORT: u16 = 9080;
pub const PROTOCOL_ID: u64 = 0;
pub const PLAYER_MOVE_SPEED: f32 = 5.0;

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_resource::<Lobby>()
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

#[derive(Debug, Default, Serialize, Deserialize, Component, Resource)]
pub struct ClientInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
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
    PlayerConnected { id: ClientId },
    PlayerDisconnected { id: ClientId },
}
