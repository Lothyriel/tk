//! Opt-in native client route. Inputs are synthetic Bevy events, not OS input.
//! Match transitions are real snapshots from a separately started server.
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::MouseButtonInput,
    },
    prelude::*,
    ui::UiGlobalTransform,
    ui_widgets::Button,
    window::{CursorGrabMode, CursorMoved, CursorOptions, PrimaryWindow, WindowEvent},
};
use common::{
    LocalInput, PlayerId,
    raid::{RaidPhase, RaidStatus},
};

use crate::{ClientSystems, menu::Menu, sync::ClientSession};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Resource)]
pub(crate) enum Probe {
    Mouse,
    Keyboard,
    Mixed,
}

impl Probe {
    pub(crate) fn requested() -> Option<Self> {
        std::env::args().find_map(|argument| {
            argument.strip_prefix("--arena-menu-probe=").map(|value| {
                Self::parse(value).expect("use --arena-menu-probe=mouse|keyboard|mixed")
            })
        })
    }

    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "mouse" => Self::Mouse,
            "keyboard" => Self::Keyboard,
            "mixed" => Self::Mixed,
            _ => return None,
        })
    }

    fn mouse(self, step: Step) -> bool {
        match self {
            Self::Mouse => true,
            Self::Keyboard => false,
            Self::Mixed => matches!(
                step,
                Step::Mode | Step::CloseBench | Step::Cancel | Step::Leave | Step::BenchAgain
            ),
        }
    }
}

impl Plugin for Probe {
    fn build(&self, app: &mut App) {
        app.insert_resource(*self)
            .init_resource::<Route>()
            .add_systems(Update, drive.after(ClientSystems::Present));
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Step {
    #[default]
    Mode,
    Bench,
    CloseBench,
    Ready,
    Cancel,
    ReadyAgain,
    Leave,
    BenchAgain,
    CloseAgain,
    Reenter,
    Done,
}

impl Step {
    fn label(self) -> Option<&'static str> {
        Some(match self {
            Self::Mode => "Mode: Raid",
            Self::Bench | Self::BenchAgain => "Weapon workbench",
            Self::CloseBench | Self::CloseAgain => "Back to lobby",
            Self::Ready | Self::ReadyAgain | Self::Reenter => "Ready / deploy",
            Self::Cancel => "Cancel ready",
            Self::Leave => "Leave Arena / return to lobby",
            Self::Done => return None,
        })
    }

    fn next(self) -> Self {
        use Step::*;
        match self {
            Mode => Bench,
            Bench => CloseBench,
            CloseBench => Ready,
            Ready => Cancel,
            Cancel => ReadyAgain,
            ReadyAgain => Leave,
            Leave => BenchAgain,
            BenchAgain => CloseAgain,
            CloseAgain => Reenter,
            Reenter | Done => Done,
        }
    }

    fn sequence(self) -> u32 {
        match self {
            Self::Mode | Self::Bench | Self::CloseBench | Self::Ready => 0,
            Self::Cancel => 1,
            Self::ReadyAgain => 2,
            Self::Leave => 3,
            Self::BenchAgain | Self::CloseAgain | Self::Reenter => 4,
            Self::Done => 5,
        }
    }
}

#[derive(Resource, Default)]
struct Route {
    step: Step,
    since: f64,
    release: Option<KeyCode>,
    release_mouse: bool,
    moved_mouse: bool,
    first_raid: Option<u32>,
    checked_return_focus: bool,
}

type Widgets<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Children,
        &'static ComputedNode,
        &'static Node,
        Has<crate::menu::NavOrder>,
        Option<&'static Outline>,
    ),
    With<Button>,
>;
type KeysAndMouse<'w> = (
    MessageWriter<'w, KeyboardInput>,
    MessageWriter<'w, MouseButtonInput>,
    MessageWriter<'w, WindowEvent>,
);

fn presented_button<'a>(
    widgets: &Widgets,
    text: &'a Query<&Text>,
    prefix: &str,
) -> Option<(Entity, &'a str, bool)> {
    widgets
        .iter()
        .find_map(|(entity, children, node, style, routed, _)| {
            if !routed || style.display == Display::None {
                return None;
            }
            let label = children
                .iter()
                .filter_map(|id| text.get(id).ok())
                .find(|label| label.starts_with(prefix))?;
            Some((entity, label.as_str(), node.size().min_element() > 0.0))
        })
}

fn key_event(window: Entity, key_code: KeyCode, state: ButtonState) -> KeyboardInput {
    KeyboardInput {
        key_code,
        logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
        state,
        text: None,
        repeat: false,
        window,
    }
}

#[allow(clippy::too_many_arguments)] // Separate input and read-only presentation queries.
fn drive(
    (probe, mut route, time): (Res<Probe>, ResMut<Route>, Res<Time<Real>>),
    (session, player, input, menu): (
        Res<ClientSession>,
        Res<PlayerId>,
        Res<LocalInput>,
        Res<Menu>,
    ),
    window: Single<(Entity, &Window, &CursorOptions), With<PrimaryWindow>>,
    widgets: Widgets,
    layout: Query<&UiGlobalTransform, With<Button>>,
    text: Query<&Text>,
    (mut keyboard, mut mouse, mut window_events): KeysAndMouse,
    mut exit: MessageWriter<AppExit>,
) {
    use Step::*;
    let (window_id, window, cursor) = *window;
    let now = time.elapsed_secs_f64();
    let step = route.step;
    assert!(
        now - route.since < if step == Leave { 90.0 } else { 12.0 },
        "Arena menu probe {probe:?} timed out at {step:?}. Use an Arena-capable build, a fresh solo server and a focused client."
    );
    if let Some(key) = route.release.take() {
        keyboard.write(key_event(window_id, key, ButtonState::Released));
        return;
    }
    if std::mem::take(&mut route.release_mouse) {
        let release = MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Released,
            window: window_id,
        };
        mouse.write(release);
        window_events.write(WindowEvent::MouseButtonInput(release));
        return;
    }
    if !window.focused {
        return;
    }
    assert_eq!(
        input.lobby_sequence,
        step.sequence(),
        "unexpected extra or missing command before {step:?}"
    );
    assert_eq!(
        input.fire_pressed_sequence, 0,
        "menu click leaked into gameplay"
    );
    assert!(!input.fire && !input.aim);
    let Some(actor) = session.local(player.0) else {
        return;
    };
    let button = |prefix: &str| {
        presented_button(&widgets, &text, prefix)
            .and_then(|(entity, _, laid_out)| laid_out.then_some(entity))
    };
    match step {
        Bench | Ready | ReadyAgain | BenchAgain | Reenter
            if session.raid.phase != RaidPhase::Lobby
                || actor.participant.ready
                || actor.participant.last_command != step.sequence() =>
        {
            return;
        }
        Cancel
            if session.raid.phase != RaidPhase::Countdown
                || !actor.participant.ready
                || actor.participant.last_command != 1 =>
        {
            return;
        }
        Leave => {
            if session.raid.phase != RaidPhase::Active {
                return;
            }
            route.first_raid.get_or_insert(session.raid.raid_id);
            // Mouse-only does not press Escape to pause. Wait for the real bot
            // to cause the death screen, which also exposes Leave.
            if actor.participant.status != RaidStatus::Dead {
                return;
            }
        }
        Done => {
            if session.raid.phase != RaidPhase::Active {
                return;
            }
            assert!(session.raid.raid_id > route.first_raid.expect("first Arena deployment"));
            assert_eq!(actor.participant.last_command, 5);
            info!(
                "ARENA MENU PROBE PASS {probe:?}: synthetic Bevy input, two native server deployments"
            );
            exit.write(AppExit::Success);
            return;
        }
        _ => (),
    }
    if matches!(
        step,
        Bench | Ready | ReadyAgain | BenchAgain | Reenter | Cancel
    ) {
        // Present changes display and text in Update. On the first frame
        // back from Arena, PostUpdate layout hasn't sized the buttons yet.
        let Some((_, label, laid_out)) = presented_button(&widgets, &text, "Mode: ") else {
            return;
        };
        assert_eq!(
            label, "Mode: Arena Solo (change)",
            "Arena Solo selection was lost at {step:?}"
        );
        if !laid_out {
            return;
        }
    }
    if matches!(step, Ready | Reenter) {
        let Some(bench) = button("Weapon workbench") else {
            return;
        };
        // Assert on the first frame following the workbench transition; after
        // recording this, keyboard Tab may legitimately move focus elsewhere.
        if !route.checked_return_focus {
            assert_eq!(
                menu.focused_widget(),
                Some(bench),
                "focus was not restored from workbench"
            );
            route.checked_return_focus = true;
        }
    }
    let Some(target) = step.label().and_then(button) else {
        return;
    };
    assert!(menu.owns_input());
    assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    if probe.mouse(step) {
        if !route.moved_mouse {
            let transform = layout.get(target).expect("laid-out menu button");
            window_events.write(WindowEvent::CursorMoved(CursorMoved {
                window: window_id,
                position: (*transform * Vec2::ZERO) / window.scale_factor(),
                delta: None,
            }));
            route.moved_mouse = true;
            return;
        }
        let press = MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window: window_id,
        };
        mouse.write(press);
        window_events.write(WindowEvent::MouseButtonInput(press));
        route.moved_mouse = false;
        route.release_mouse = true;
    } else {
        let focused = menu.focused_widget() == Some(target);
        if focused {
            assert!(
                widgets
                    .get(target)
                    .unwrap()
                    .5
                    .is_some_and(|outline| outline.color != Color::NONE)
            );
        }
        let key = if focused {
            KeyCode::Enter
        } else {
            KeyCode::Tab
        };
        keyboard.write(key_event(window_id, key, ButtonState::Pressed));
        route.release = Some(key);
        if !focused {
            return;
        }
    }
    info!(
        "ARENA MENU PROBE {probe:?} {step:?} sequence={}",
        step.sequence()
    );
    route.step = step.next();
    route.since = now;
    route.checked_return_focus = false;
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn arena_menu_return_label_is_visible_before_buttons_are_laid_out() {
        let mut world = World::new();
        let mode = world
            .spawn((
                crate::menu::widget(0),
                Node {
                    display: Display::None,
                    ..default()
                },
                ComputedNode::default(),
                children![(Text::new("Mode: Arena Solo (change)"),)],
            ))
            .id();
        let observe = |widgets: Widgets, text: Query<&Text>| {
            presented_button(&widgets, &text, "Mode: ")
                .map(|(entity, label, laid_out)| (entity, label.to_owned(), laid_out))
        };
        assert_eq!(world.run_system_once(observe).unwrap(), None);
        // Presentation has restored the lobby, but layout still has the
        // previous (hidden) frame's geometry.
        world.get_mut::<Node>(mode).unwrap().display = Display::Flex;
        assert_eq!(
            world.run_system_once(observe).unwrap(),
            Some((mode, "Mode: Arena Solo (change)".into(), false)),
        );
        world.get_mut::<ComputedNode>(mode).unwrap().size = Vec2::splat(32.0);
        assert_eq!(
            world.run_system_once(observe).unwrap(),
            Some((mode, "Mode: Arena Solo (change)".into(), true)),
        );
    }
}
