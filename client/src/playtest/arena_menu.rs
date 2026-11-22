//! Opt-in native client route. Inputs are synthetic Bevy events, not OS input.
//! Match transitions are real snapshots from a separately started server.
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::MouseButtonInput,
    },
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
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
        let mut route = Route::default();
        let review_scale = std::env::var("TK_PREFERENCES_REVIEW_SCALE").ok();
        if let Ok(mode) = std::env::var("TK_PREFERENCES_FLOW") {
            assert_eq!(*self, Self::Keyboard);
            assert!(std::env::var_os("TK_PREFERENCES_SCREENSHOT").is_none());
            let path = std::path::PathBuf::from(
                std::env::var_os("TK_PREFERENCES_PATH").expect("isolated preferences path"),
            );
            route.step = match mode.as_str() {
                "edit" => {
                    assert!(!path.exists());
                    Step::EditPreferences(0)
                }
                "pause-edit" => {
                    assert!(!path.exists());
                    route.pause_flow = true;
                    Step::PreferencesDeploy
                }
                "reload" => {
                    assert!(path.is_file());
                    Step::ReloadPreferences(false)
                }
                _ => panic!("use TK_PREFERENCES_FLOW=edit, pause-edit or reload"),
            };
            route.flow_path = Some(path);
        }
        if let Some(path) = std::env::var_os("TK_PREFERENCES_SCREENSHOT") {
            assert_eq!(
                *self,
                Self::Keyboard,
                "Preferences review uses the keyboard probe"
            );
            let path = std::path::PathBuf::from(path);
            assert!(!path.exists(), "Preferences review needs a fresh PNG path");
            route.preferences = Some(path);
            route.step = Step::OpenPreferences;
        } else {
            assert!(
                review_scale.is_none(),
                "review scale requires the Preferences screenshot probe"
            );
        }
        if let Some(value) = review_scale {
            let scale: f32 = value.parse().expect("numeric Preferences review scale");
            assert!(
                (1.0..=2.0).contains(&scale),
                "Preferences review scale must be 1.0–2.0"
            );
            // This opt-in route only opens and observes the screen. Leave
            // persisted preferences and the draft unchanged.
            app.insert_resource(UiScale(scale));
        }
        app.insert_resource(*self)
            .insert_resource(route)
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
    OpenPreferences,
    CapturePreferences,
    PreferencesDeploy,
    PreferencesPause,
    EditPreferences(usize),
    ReloadPreferences(bool),
}

// Activate only real menu widgets; None represents an explicit Esc or
// the final observation rather than an activation.
const EDIT_ACTIONS: [Option<&str>; 14] = [
    Some("Preferences"),
    Some("+ Vertical FOV"),
    Some("Revert unapplied"),
    Some("+ Vertical FOV"),
    None,
    Some("Preferences"),
    Some("+ Vertical FOV"),
    Some("Back (discard draft)"),
    Some("Preferences"),
    Some("+ Vertical FOV"),
    Some("UI scale:"),
    Some("Apply and save"),
    Some("Back (discard draft)"),
    None,
];

impl Step {
    fn label(self) -> Option<&'static str> {
        Some(match self {
            Self::Mode => "Mode: Raid",
            Self::Bench | Self::BenchAgain => "Weapon workbench",
            Self::CloseBench | Self::CloseAgain => "Back to lobby",
            Self::Ready | Self::ReadyAgain | Self::Reenter | Self::PreferencesDeploy => {
                "Ready / deploy"
            }
            Self::Cancel => "Cancel ready",
            Self::Leave => "Leave Arena / return to lobby",
            Self::OpenPreferences => "Preferences",
            Self::EditPreferences(index) => return EDIT_ACTIONS[index],
            Self::ReloadPreferences(false) => "Preferences",
            Self::ReloadPreferences(true) => return None,
            Self::Done | Self::CapturePreferences | Self::PreferencesPause => return None,
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
            OpenPreferences | CapturePreferences => CapturePreferences,
            EditPreferences(index) => EditPreferences(index + 1),
            ReloadPreferences(_) => ReloadPreferences(true),
            PreferencesDeploy => PreferencesPause,
            PreferencesPause => EditPreferences(0),
        }
    }

    fn sequence(self) -> u32 {
        match self {
            Self::Mode | Self::Bench | Self::CloseBench | Self::Ready | Self::PreferencesDeploy => {
                0
            }
            Self::PreferencesPause => 1,
            Self::Cancel => 1,
            Self::ReadyAgain => 2,
            Self::Leave => 3,
            Self::BenchAgain | Self::CloseAgain | Self::Reenter => 4,
            Self::Done => 5,
            Self::OpenPreferences
            | Self::CapturePreferences
            | Self::EditPreferences(_)
            | Self::ReloadPreferences(_) => 0,
        }
    }
}

#[derive(Resource, Default)]
struct Route {
    step: Step,
    since: f64,
    release: Option<KeyCode>,
    release_mouse: Option<Vec2>,
    moved_mouse: bool,
    first_raid: Option<u32>,
    checked_return_focus: bool,
    preferences: Option<std::path::PathBuf>,
    capture_requested: bool,
    flow_path: Option<std::path::PathBuf>,
    pause_flow: bool,
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
    mut commands: Commands,
    (probe, mut route, time): (Res<Probe>, ResMut<Route>, Res<Time<Real>>),
    scale: Res<UiScale>,
    preferences: Res<crate::preferences::Preferences>,
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
    editor: Res<crate::loadout::Editor>,
    (mut keyboard, mut mouse, mut window_events): KeysAndMouse,
    mut exit: MessageWriter<AppExit>,
) {
    use Step::*;
    let (window_id, window, cursor) = *window;
    let now = time.elapsed_secs_f64();
    let step = route.step;
    let sequence =
        step.sequence() + u32::from(route.pause_flow && matches!(step, EditPreferences(_)));
    assert!(
        now - route.since < if step == Leave { 90.0 } else { 12.0 },
        "Arena menu probe {probe:?} timed out at {step:?}; workbench visible={}; target={:?}. Use an Arena-capable build, a fresh solo server and a focused client.",
        editor.visible,
        step.label()
            .and_then(|label| presented_button(&widgets, &text, label)),
    );
    if let Some(key) = route.release.take() {
        keyboard.write(key_event(window_id, key, ButtonState::Released));
        return;
    }
    if let Some(position) = route.release_mouse.take() {
        let release = MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Released,
            window: window_id,
        };
        window_events.write(WindowEvent::CursorMoved(CursorMoved {
            window: window_id,
            position,
            delta: None,
        }));
        mouse.write(release);
        window_events.write(WindowEvent::MouseButtonInput(release));
        return;
    }
    if !window.focused {
        return;
    }
    assert_eq!(
        input.lobby_sequence, sequence,
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
    if step == CapturePreferences {
        let path = route.preferences.clone().expect("Preferences capture path");
        if route.capture_requested {
            if path.is_file() {
                info!("PREFERENCES REVIEW saved {}", path.display());
                exit.write(AppExit::Success);
            }
            return;
        }
        if presented_button(&widgets, &text, "Apply and save").is_none()
            || presented_button(&widgets, &text, "Back (discard draft)").is_none()
            || now - route.since < 0.2
        {
            return;
        }
        info!(
            "PREFERENCES REVIEW viewport={}x{} ui_scale={} focused={:?}",
            window.physical_width(),
            window.physical_height(),
            scale.0,
            menu.focused_widget()
        );
        for (entity, children, node, style, routed, outline) in &widgets {
            if !routed || style.display == Display::None || node.size() == Vec2::ZERO {
                continue;
            }
            let label = children.iter().find_map(|child| text.get(child).ok());
            let center = layout
                .get(entity)
                .map(|transform| *transform * Vec2::ZERO)
                .ok();
            let inside = center.is_some_and(|center| {
                let half = node.size() * 0.5;
                (center - half).cmpge(Vec2::ZERO).all()
                    && (center + half)
                        .cmple(Vec2::new(
                            window.physical_width() as f32,
                            window.physical_height() as f32,
                        ))
                        .all()
            });
            info!(
                "PREFERENCES REVIEW {entity:?} label={label:?} center={center:?} size={:?} inside_viewport={inside} outlined={}",
                node.size(),
                outline.is_some_and(|outline| outline.color != Color::NONE)
            );
        }
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
        route.capture_requested = true;
        return;
    }
    if step == OpenPreferences
        && (session.raid.phase != RaidPhase::Lobby || actor.participant.ready)
    {
        return;
    }
    let button = |prefix: &str| {
        presented_button(&widgets, &text, prefix)
            .and_then(|(entity, _, laid_out)| laid_out.then_some(entity))
    };
    if step == PreferencesPause {
        if session.raid.phase != RaidPhase::Active || !session.playing(player.0) {
            return;
        }
        assert_eq!(actor.participant.last_command, sequence);
        route.first_raid = Some(session.raid.raid_id);
        keyboard.write(key_event(window_id, KeyCode::Escape, ButtonState::Pressed));
        route.release = Some(KeyCode::Escape);
        route.step = step.next();
        route.since = now;
        return;
    }
    if let Some(path) = route
        .flow_path
        .as_ref()
        .filter(|_| step != PreferencesDeploy)
    {
        if route.pause_flow {
            assert_eq!(session.raid.phase, RaidPhase::Active);
            assert_eq!(Some(session.raid.raid_id), route.first_raid);
            assert!(session.playing(player.0) && menu.paused() && menu.owns_input());
        } else if session.raid.phase != RaidPhase::Lobby {
            return;
        } else {
            assert!(!actor.participant.ready);
        }
        assert_eq!(actor.participant.last_command, sequence);
        let fov = || presented_button(&widgets, &text, "- Vertical FOV:").map(|(_, text, _)| text);
        let scale_text = || presented_button(&widgets, &text, "UI scale:").map(|(_, text, _)| text);
        let applied = matches!(step, EditPreferences(12..) | ReloadPreferences(_));
        let expected = crate::preferences::Preferences {
            fov: if applied { 75 } else { 70 },
            ui_scale_percent: if applied { 125 } else { 100 },
            ..default()
        };
        assert_eq!(*preferences, expected);
        assert_eq!(scale.0, expected.ui_scale());
        if applied {
            let saved = std::fs::read(path).expect("saved preferences");
            let saved = serde_json::from_slice::<crate::preferences::Preferences>(&saved)
                .expect("valid saved preferences");
            assert_eq!(saved, expected);
        } else {
            assert!(!path.exists(), "a draft unexpectedly saved");
        }
        match step {
            EditPreferences(2 | 4 | 7 | 10 | 11 | 12) => {
                assert_eq!(fov(), Some("- Vertical FOV: 75 deg"));
            }
            EditPreferences(3 | 6 | 9) => {
                assert_eq!(fov(), Some("- Vertical FOV: 70 deg"));
            }
            _ => {}
        }
        if matches!(step, EditPreferences(5 | 8 | 13)) {
            let Some(settings) = button("Preferences") else {
                return;
            };
            assert_eq!(menu.focused_widget(), Some(settings));
        }
        if step == EditPreferences(4) {
            keyboard.write(key_event(window_id, KeyCode::Escape, ButtonState::Pressed));
            route.release = Some(KeyCode::Escape);
            route.step = step.next();
            route.since = now;
            return;
        }
        if step == ReloadPreferences(true) {
            assert_eq!(fov(), Some("- Vertical FOV: 75 deg"));
            assert_eq!(scale_text(), Some("UI scale: 125% (cycle)"));
        }
        if matches!(step, EditPreferences(13) | ReloadPreferences(true)) {
            info!(
                "PREFERENCES FLOW PASS {step:?}: FOV 75, scale 125, pause={}, lobby_sequence={sequence}, no aim or fire",
                route.pause_flow
            );
            exit.write(AppExit::Success);
            return;
        }
    }
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
        let transform = layout.get(target).expect("laid-out menu button");
        let position = (*transform * Vec2::ZERO) / window.scale_factor();
        // Window events from the compositor may move the pointer between
        // frames. Associate both synthetic button edges with the intended
        // coordinates, while still using the real picking backend.
        window_events.write(WindowEvent::CursorMoved(CursorMoved {
            window: window_id,
            position,
            delta: None,
        }));
        if !route.moved_mouse {
            debug!(
                "ARENA MENU PROBE pointer {step:?} target={target:?} position={position:?} window={:?}",
                window.size()
            );
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
        route.release_mouse = Some(position);
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
