//! Lobby-only weapon editor. Drafts use the shared resolver; rendering and
//! deployment consume server-confirmed configurations.
use crate::{ClientSystems, sync::ClientSession};
use bevy::prelude::*;
use bevy_renet2::prelude::RenetClient;
use common::{PlayerId, data, replication::CONFIGURATION_CHANNEL};

mod editor;
pub(crate) mod preview;
#[cfg(test)]
mod tests;
pub(crate) mod ui;
pub(crate) use editor::Editor;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EditorInput;

pub struct Plugin;

#[cfg(test)]
pub(crate) fn setup_test_widgets(app: &mut App) {
    app.insert_resource(preview::PreviewImage(Handle::default()))
        .init_resource::<Editor>()
        .init_resource::<preview::Preview>()
        .add_systems(Startup, ui::setup)
        .add_systems(
            Update,
            synchronize
                .in_set(ClientSystems::Input)
                .before(crate::menu::MenuInput),
        )
        .add_systems(
            Update,
            interact
                .in_set(ClientSystems::Input)
                .after(crate::menu::MenuInput)
                .before(crate::input::InputCapture),
        )
        .add_systems(Update, ui::refresh.in_set(ClientSystems::Present));
}
impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Editor>()
            .init_resource::<preview::Preview>()
            .add_systems(Startup, (preview::setup, ui::setup).chain())
            .add_systems(
                Update,
                synchronize
                    .before(crate::menu::MenuInput)
                    .in_set(ClientSystems::Input),
            )
            .add_systems(
                Update,
                interact
                    .after(crate::menu::MenuInput)
                    .before(crate::input::InputCapture)
                    .in_set(EditorInput)
                    .in_set(ClientSystems::Input),
            )
            .add_systems(
                Update,
                (
                    preview::update.before(crate::render::voxel::InstantiateModels),
                    ui::refresh,
                )
                    .chain()
                    .in_set(ClientSystems::Present),
            );
    }
}

fn synchronize(mut editor: ResMut<Editor>, session: Res<ClientSession>, id: Res<PlayerId>) {
    editor.sync(&session, id.0);
}

fn interact(
    mut activations: MessageReader<crate::menu::Activation>,
    buttons: Query<&ui::Action>,
    mut editor: ResMut<Editor>,
    (session, id, time): (Res<ClientSession>, Res<PlayerId>, Res<Time<Real>>),
    mut client: ResMut<RenetClient>,
    menu: Option<Res<crate::menu::Menu>>,
) {
    for activation in activations.read() {
        let Ok(action) = buttons.get(activation.0) else {
            continue;
        };
        if !editor.visible {
            continue;
        }
        if *action == ui::Action::Close {
            editor.visible = false;
            continue;
        }
        if !editor.editable(&session, id.0)
            || menu.as_ref().is_some_and(|menu| menu.awaiting_lobby())
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
