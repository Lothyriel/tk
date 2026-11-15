//! Lobby-only weapon editor. Drafts use the shared resolver; rendering and
//! deployment consume server-confirmed configurations.
use crate::{ClientSystems, sync::ClientSession};
use bevy::prelude::*;
use bevy_renet2::prelude::RenetClient;
use common::{PlayerId, data, replication::CONFIGURATION_CHANNEL};

mod editor;
mod preview;
#[cfg(test)]
mod tests;
pub(crate) mod ui;
pub(crate) use editor::Editor;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EditorInput;

pub struct Plugin;
impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Editor>()
            .add_systems(Startup, (preview::setup, ui::setup).chain())
            .add_systems(
                Update,
                (synchronize, interact)
                    .chain()
                    .after(crate::input::InputCapture)
                    .in_set(EditorInput)
                    .in_set(ClientSystems::Input),
            )
            .add_systems(
                Update,
                (ui::refresh, preview::update).in_set(ClientSystems::Present),
            );
    }
}

fn synchronize(
    mut editor: ResMut<Editor>,
    session: Res<ClientSession>,
    id: Res<PlayerId>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    editor.sync(&session, id.0);
    if keys.just_pressed(KeyCode::KeyV)
        && matches!(
            session.raid.phase,
            common::raid::RaidPhase::Lobby | common::raid::RaidPhase::Countdown
        )
    {
        editor.visible = !editor.visible;
    }
}

fn interact(
    buttons: Query<&ui::Action, Added<bevy::ui::Pressed>>,
    mut editor: ResMut<Editor>,
    session: Res<ClientSession>,
    id: Res<PlayerId>,
    mut client: ResMut<RenetClient>,
    time: Res<Time<Real>>,
) {
    for action in &buttons {
        if !editor.visible {
            continue;
        }
        if *action == ui::Action::Close {
            editor.visible = false;
            continue;
        }
        if !editor.editable(&session, id.0)
            || !client.is_connected()
            || time.elapsed_secs_f64() - session.last_received > 2.0
        {
            continue;
        }
        match *action {
            ui::Action::Slot => editor.switch_slot(),
            ui::Action::Part(row) => editor.cycle_part(row),
            ui::Action::Position(row) => editor.cycle_position(row),
            ui::Action::Reserve(index) => editor.cycle_reserve(index),
            ui::Action::Reset => editor.reset(),
            ui::Action::Apply => {
                if client.can_send_message(
                    CONFIGURATION_CHANNEL,
                    common::replication::MAX_CONFIGURATION_BYTES,
                ) && let Some(command) = editor.command(&session, id.0)
                {
                    client.send_message(CONFIGURATION_CHANNEL, data::encode(&command));
                }
            }
            ui::Action::Close => (),
        }
    }
}
