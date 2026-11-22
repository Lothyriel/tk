//! Observe client ordering without moving any input or receive systems.
use bevy::{
    diagnostic::FrameCount,
    input::{
        ButtonState, InputSystems,
        keyboard::{Key, KeyboardInput},
    },
    ui::UiSystems,
};

use super::*;
use crate::{ClientSystems, input::InventoryInput, sync::ClientSession};
use common::PlayerId;

pub(crate) struct CloseAckPlugin;

impl Plugin for CloseAckPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CloseAck>()
            .add_systems(PreUpdate, keys.before(InputSystems))
            .add_systems(
                Update,
                receive_state
                    .after(ClientSystems::Receive)
                    .before(ClientSystems::Input),
            )
            .add_systems(
                Update,
                submitted
                    .after(InventoryInput)
                    .before(ClientSystems::Present),
            )
            .add_systems(PostUpdate, observe.after(UiSystems::PostLayout));
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Stage {
    #[default]
    Waiting,
    SeenPending,
    SentClose,
    Closed,
    ReadyToReopen,
    SentReopen,
    Finished,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ordering {
    Close,
    Ack,
    Delta,
}

fn classify(ack: bool, delta: bool) -> Ordering {
    match (ack, delta) {
        (true, _) => Ordering::Ack,
        (false, true) => Ordering::Delta,
        (false, false) => Ordering::Close,
    }
}

#[derive(Resource, Default)]
pub(crate) struct CloseAck {
    stage: Stage,
    pending: Option<Pending>,
    close_order: Option<Ordering>,
    saw_ack: bool,
    saw_delta: bool,
    release_tab: bool,
    frame: u32,
}

impl CloseAck {
    pub(crate) fn finished(&self) -> bool {
        self.stage == Stage::Finished
    }

    pub(crate) fn on_tab_close(&mut self, panel: &Panel, inventory: &State) {
        if self.stage != Stage::SentClose {
            return;
        }
        let pending = self.pending.unwrap();
        let order = classify(self.saw_ack, self.saw_delta);
        assert!(panel.drag.is_none() && panel.inspection.is_none());
        if order == Ordering::Close {
            assert_eq!(panel.pending.map(|p| p.operation), Some(pending.operation));
            assert_eq!(inventory.pending_operation_id(), Some(pending.operation));
            assert_eq!(inventory.outcome(pending.operation), None);
        }
        self.close_order = Some(order);
        self.stage = Stage::Closed;
        self.trace("tab_close", pending);
    }

    fn trace(&self, stage: &str, pending: Pending) {
        info!(
            "GRID CLOSE ACK {stage} raid={} session={} op={} item={} revision={} frame={}",
            pending.session.raid_epoch,
            pending.session.session_epoch,
            pending.operation,
            pending.item.0,
            pending.version.revision,
            self.frame
        );
    }
}

fn keys(
    mut route: ResMut<CloseAck>,
    frame: Res<FrameCount>,
    window: Single<(Entity, &Window), With<PrimaryWindow>>,
    mut keyboard: MessageWriter<KeyboardInput>,
) {
    route.frame = frame.0;
    let (window, settings) = *window;
    let mut emit = |key_code, logical_key, state| {
        keyboard.write(KeyboardInput {
            key_code,
            logical_key,
            state,
            window,
            repeat: false,
            text: None,
        });
    };
    if std::mem::take(&mut route.release_tab) {
        emit(KeyCode::Tab, Key::Tab, ButtonState::Released);
        return;
    }
    match route.stage {
        Stage::SeenPending => {
            emit(KeyCode::Enter, Key::Enter, ButtonState::Released);
            route.stage = Stage::SentClose;
        }
        Stage::ReadyToReopen => route.stage = Stage::SentReopen,
        _ => return,
    }
    assert!(settings.focused, "GRID CLOSE ACK lost window focus");
    emit(KeyCode::Tab, Key::Tab, ButtonState::Pressed);
    route.release_tab = true;
}

type Live<'w> = (
    Res<'w, State>,
    Res<'w, Panel>,
    Res<'w, ClientSession>,
    Res<'w, PlayerId>,
);

fn assert_session(
    route: &CloseAck,
    pending: Pending,
    inventory: &State,
    session: &ClientSession,
    player: &PlayerId,
) {
    assert!(
        session.playing(player.0),
        "GRID CLOSE ACK left the live raid"
    );
    assert_eq!(inventory.grid_session(), Some(pending.session));
    assert_eq!(
        inventory.last_operation_id(),
        pending.operation,
        "GRID CLOSE ACK operation replayed"
    );
    assert!(!route.finished());
}

fn receive_state(mut route: ResMut<CloseAck>, (inventory, _, session, player): Live) {
    let Some(pending) = route.pending.filter(|_| !route.finished()) else {
        return;
    };
    assert_session(&route, pending, &inventory, &session, &player);
    let grid = inventory
        .owned_grid(session.raid.raid_id, player.0)
        .expect("confirmed grid");
    // Both transitions can come from the same receive batch. Their trace
    // order within this sample is not packet or server event order.
    if grid.version.revision != pending.version.revision && !route.saw_delta {
        assert_eq!(grid.version.revision, pending.version.revision + 1);
        let item = &grid
            .items
            .iter()
            .find(|(id, _)| *id == pending.item)
            .expect("same moved item")
            .1;
        assert_eq!(item.placement, pending.target);
        route.saw_delta = true;
        route.trace("delta_applied", pending);
    }
    if let Some(result) = inventory.outcome(pending.operation)
        && !route.saw_ack
    {
        assert_eq!(result, OperationResult::Committed);
        route.saw_ack = true;
        route.trace("ack_applied", pending);
    }
}

fn submitted(mut route: ResMut<CloseAck>, (inventory, panel, session, player): Live) {
    if route.pending.is_none()
        && let Some(pending) = panel.pending
    {
        assert!(matches!(pending.expected, Expected::Move));
        assert_session(&route, pending, &inventory, &session, &player);
        assert_eq!(inventory.pending_operation_id(), Some(pending.operation));
        route.pending = Some(pending);
        route.trace("submitted", pending);
    }
}

fn observe(
    mut route: ResMut<CloseAck>,
    (inventory, panel, session, player): Live,
    root: Single<&ComputedNode, With<GridRoot>>,
    ghosts: Query<(&GhostKind, &ComputedNode)>,
) {
    let laid_out = root.size().min_element() > 0.0;
    match route.stage {
        Stage::Waiting => {
            let Some(pending) = route.pending else { return };
            if !panel.open
                || !laid_out
                || panel.pending.map(|p| p.operation) != Some(pending.operation)
                || !ghosts.iter().any(|(kind, node)| {
                    matches!(kind, GhostKind::Pending) && node.size().min_element() > 0.0
                })
            {
                return;
            }
            assert!(matches!(pending.expected, Expected::Move));
            assert_session(&route, pending, &inventory, &session, &player);
            assert_eq!(inventory.pending_operation_id(), Some(pending.operation));
            assert_eq!(inventory.outcome(pending.operation), None);
            assert_eq!(
                inventory
                    .owned_grid(session.raid.raid_id, player.0)
                    .unwrap()
                    .version,
                pending.version
            );
            route.stage = Stage::SeenPending;
            route.trace("pending_presented", pending);
        }
        Stage::SentClose => panic!("GRID CLOSE ACK Tab did not reach the panel close path"),
        Stage::Closed => {
            assert!(!panel.open && !laid_out);
            if route.saw_ack && route.saw_delta {
                route.stage = Stage::ReadyToReopen;
            }
        }
        Stage::SentReopen => {
            let pending = route.pending.unwrap();
            assert_session(&route, pending, &inventory, &session, &player);
            assert!(panel.open && laid_out && ghosts.is_empty());
            assert!(panel.pending.is_none() && panel.drag.is_none());
            assert!(panel.cue.is_none());
            assert!(panel.feedback.is_none());
            assert_eq!(inventory.pending_operation_id(), None);
            let grid = inventory
                .owned_grid(session.raid.raid_id, player.0)
                .expect("reopened confirmed grid");
            assert_eq!(grid.version.revision, pending.version.revision + 1);
            assert!(
                grid.items
                    .iter()
                    .any(|(id, item)| *id == pending.item && item.placement == pending.target)
            );
            route.trace("reopened", pending);
            route.trace(
                match route.close_order.unwrap() {
                    Ordering::Close => "CLOSE_BEFORE_ACK",
                    Ordering::Ack => "ACK_FIRST_NOT_OBSERVED",
                    Ordering::Delta => "DELTA_FIRST_NOT_OBSERVED",
                },
                pending,
            );
            route.stage = Stage::Finished;
        }
        _ => (),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_grid_close_ack_needs_neither_reply_nor_revision_before_close() {
        assert_eq!(classify(false, false), Ordering::Close);
        assert_eq!(classify(true, false), Ordering::Ack);
        assert_eq!(classify(true, true), Ordering::Ack);
        assert_eq!(classify(false, true), Ordering::Delta);
    }

    #[test]
    fn inventory_grid_close_ack_uses_normal_keyboard_schedule_for_both_tabs() {
        let mut app = App::new();
        app.add_plugins(bevy::input::InputPlugin)
            .init_resource::<FrameCount>()
            .init_resource::<CloseAck>()
            .add_systems(PreUpdate, keys.before(InputSystems));
        app.world_mut().spawn((
            PrimaryWindow,
            Window {
                focused: true,
                ..default()
            },
        ));
        app.update();
        let tab = |app: &App| {
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(KeyCode::Tab)
        };
        assert!(!tab(&app));
        app.world_mut().resource_mut::<CloseAck>().stage = Stage::SeenPending;
        app.update();
        assert!(tab(&app));
        assert_eq!(app.world().resource::<CloseAck>().stage, Stage::SentClose);
        app.update();
        assert!(!tab(&app));
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_released(KeyCode::Tab)
        );
        app.world_mut().resource_mut::<CloseAck>().stage = Stage::ReadyToReopen;
        app.update();
        assert!(tab(&app));
        assert_eq!(app.world().resource::<CloseAck>().stage, Stage::SentReopen);
    }
}
