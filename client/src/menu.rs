//! One foreground widget surface owns activation before gameplay input is sampled.
use bevy::{
    picking::{events::PointerClick, pointer::PointerButton},
    prelude::*,
    ui::{ScrollPosition, UiGlobalTransform},
    ui_widgets::Button,
    window::PrimaryWindow,
};
use bevy_renet2::prelude::RenetClient;
use common::{
    EquipmentSlot, LocalInput, PlayerId, WeaponKind,
    combat::*,
    raid::{MatchMode, RaidPhase, RaidStatus, SIMULATION_HZ},
};

use crate::connection::{Connection, Request};
use crate::preferences::{Preferences, Store};
use crate::{ClientSystems, input::inventory_panel::Panel, loadout, sync::ClientSession};

pub(crate) struct Plugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct MenuInput;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        let (store, preferences) = if cfg!(test) {
            (Store::default(), Preferences::default())
        } else {
            Store::open()
        };
        app.insert_resource(store)
            .insert_resource(preferences)
            .insert_resource(UiScale(preferences.ui_scale()))
            .insert_resource(bevy::audio::GlobalVolume::new(preferences.volume()))
            .init_resource::<Menu>()
            .add_message::<Activation>()
            .add_message::<PointerActivation>()
            .add_observer(record_activation)
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                route
                    .after(crate::input::InventoryInput)
                    .before(crate::input::InputCapture)
                    .in_set(MenuInput)
                    .in_set(ClientSystems::Input),
            )
            .add_systems(Update, present.in_set(ClientSystems::Present));
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Surface {
    Connection,
    Lobby,
    Bench,
    Playing,
    Inventory,
    Pause,
    Settings,
    Waiting,
    ArenaWaiting,
    ArenaPause,
    Debrief,
}

#[derive(Resource)]
pub(crate) struct Menu {
    surface: Surface,
    paused: bool,
    selected: Option<Entity>,
    lobby_focus: Option<Entity>,
    pending: Option<u32>,
    transitioning_to_play: bool,
    settings: Option<Preferences>,
    settings_focus: Option<Entity>,
    settings_status: &'static str,
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            surface: Surface::Connection,
            paused: false,
            selected: None,
            lobby_focus: None,
            pending: None,
            transitioning_to_play: false,
            settings: None,
            settings_focus: None,
            settings_status: "",
        }
    }
}

impl Menu {
    pub(crate) fn owns_input(&self) -> bool {
        self.surface != Surface::Playing
    }

    pub(crate) fn blocks_gameplay(&self) -> bool {
        self.owns_input() || self.transitioning_to_play
    }

    pub(crate) fn paused(&self) -> bool {
        self.paused
    }

    pub(crate) fn awaiting_lobby(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn focused_widget(&self) -> Option<Entity> {
        self.selected
    }

    pub(crate) fn replaces_hud(&self) -> bool {
        !matches!(self.surface, Surface::Playing | Surface::Inventory)
    }
}

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    Ready,
    Bench,
    Mode,
    Primary,
    Ammo,
    Vest,
    Helmet,
    Return,
    Resume,
    LeaveArena,
    Quit,
    Settings,
    FovLess,
    FovMore,
    MouseLess,
    MouseMore,
    ReloadKey,
    UiScaleCycle,
    VolumeCycle,
    Defaults,
    Revert,
    ApplySettings,
    BackSettings,
    Reconnect,
    Disconnect,
}

impl Action {
    fn changes_kit(self) -> bool {
        matches!(self, Self::Primary | Self::Ammo | Self::Vest | Self::Helmet)
    }
}

#[derive(Component)]
pub(crate) struct NavOrder(u16);

#[derive(Component)]
pub(crate) struct BenchScroll;

/// A projected marker focuses its normal workbench row without cycling a part.
#[derive(Component)]
pub(crate) struct BenchFocusTarget(pub Entity);

#[derive(Message)]
pub(crate) struct Activation(pub Entity);

#[derive(Message)]
struct PointerActivation {
    entity: Entity,
    focus_only: bool,
}

#[derive(Component)]
struct Root;

#[derive(Component)]
pub(crate) struct Status;

pub(crate) fn widget(order: u16) -> impl Bundle {
    (
        Button,
        NavOrder(order),
        Outline::new(Val::Px(2.0), Val::Px(-2.0), Color::NONE),
    )
}

fn record_activation(
    event: On<PointerClick>,
    buttons: Query<(Has<NavOrder>, Option<&BenchFocusTarget>)>,
    mut activations: MessageWriter<PointerActivation>,
) {
    if event.button == PointerButton::Primary
        && let Ok((button, target)) = buttons.get(event.entity)
    {
        if let Some(target) = target {
            activations.write(PointerActivation {
                entity: target.0,
                focus_only: true,
            });
        } else if button {
            activations.write(PointerActivation {
                entity: event.entity,
                focus_only: false,
            });
        }
    }
}

fn setup(mut commands: Commands) {
    commands
        .spawn((
            Root,
            GlobalZIndex(250),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(65.0),
                left: Val::Px(18.0),
                width: Val::Percent(42.0),
                min_width: Val::Px(290.0),
                max_width: Val::Px(490.0),
                padding: UiRect::all(Val::Px(16.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.018, 0.032, 0.048)),
        ))
        .with_children(|parent| {
            parent.spawn((
                Status,
                Text::default(),
                TextFont::from_font_size(17.0),
                Node {
                    grid_column: GridPlacement::span(2),
                    ..default()
                },
            ));
            for (order, action) in [
                Action::Ready,
                Action::Bench,
                Action::Mode,
                Action::Primary,
                Action::Ammo,
                Action::Vest,
                Action::Helmet,
                Action::Return,
                Action::Resume,
                Action::LeaveArena,
                Action::Reconnect,
                Action::Disconnect,
                Action::Quit,
                Action::Settings,
                Action::FovLess,
                Action::FovMore,
                Action::MouseLess,
                Action::MouseMore,
                Action::ReloadKey,
                Action::UiScaleCycle,
                Action::VolumeCycle,
                Action::Defaults,
                Action::Revert,
                Action::ApplySettings,
                Action::BackSettings,
            ]
            .into_iter()
            .enumerate()
            {
                parent
                    .spawn((
                        widget(order as u16),
                        action,
                        Node {
                            display: Display::None,
                            padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                            ..default()
                        },
                        BackgroundColor::default(),
                    ))
                    .with_child((
                        Text::default(),
                        TextFont::from_font_size(15.0),
                        Pickable::IGNORE,
                    ));
            }
        });
}

fn connection_ready(session: &ClientSession, id: u64, client: &RenetClient, now: f64) -> bool {
    client.is_connected() && session.local(id).is_some() && now - session.last_received < 2.0
}

type WidgetQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static NavOrder,
        Option<&'static Action>,
        Option<&'static loadout::ui::Action>,
    ),
>;

fn available(
    surface: Surface,
    action: Option<&Action>,
    bench: Option<&loadout::ui::Action>,
    editable: bool,
    pending: bool,
    countdown: bool,
    kit: Option<&common::catalog::WeaponKit>,
) -> (bool, bool) {
    let shown = match surface {
        Surface::Lobby => action.is_some_and(|action| {
            matches!(
                action,
                Action::Ready
                    | Action::Bench
                    | Action::Mode
                    | Action::Quit
                    | Action::Settings
                    | Action::Disconnect
            ) || action.changes_kit()
        }),
        Surface::Bench => match bench {
            Some(loadout::ui::Action::Part(row) | loadout::ui::Action::Position(row)) => {
                kit.is_some_and(|kit| kit.weapon.definition == common::WeaponId::M4A1 || *row == 0)
            }
            _ => bench.is_some(),
        },
        Surface::ArenaPause => matches!(
            action,
            Some(Action::Resume | Action::LeaveArena | Action::Settings)
        ),
        Surface::ArenaWaiting => action == Some(&Action::LeaveArena),
        Surface::Pause => matches!(
            action,
            Some(Action::Resume | Action::Quit | Action::Settings | Action::Disconnect)
        ),
        Surface::Settings => matches!(
            action,
            Some(
                Action::FovLess
                    | Action::FovMore
                    | Action::MouseLess
                    | Action::MouseMore
                    | Action::ReloadKey
                    | Action::UiScaleCycle
                    | Action::VolumeCycle
                    | Action::Defaults
                    | Action::Revert
                    | Action::ApplySettings
                    | Action::BackSettings
            )
        ),
        Surface::Debrief => matches!(
            action,
            Some(Action::Return | Action::Quit | Action::Disconnect)
        ),
        Surface::Waiting => matches!(action, Some(Action::Quit | Action::Disconnect)),
        Surface::Connection => matches!(
            action,
            Some(Action::Quit | Action::Reconnect | Action::Disconnect)
        ),
        Surface::Playing | Surface::Inventory => false,
    };
    let enabled = match action {
        Some(Action::Ready | Action::Return | Action::LeaveArena) => !pending,
        Some(Action::Bench) => !countdown,
        Some(Action::Mode) => editable,
        Some(action) if action.changes_kit() => editable,
        Some(Action::Settings) => {
            matches!(surface, Surface::Pause | Surface::ArenaPause) || editable
        }
        Some(_) => true,
        None => bench.is_some_and(|action| {
            *action == loadout::ui::Action::Close || (editable && kit.is_some())
        }),
    };
    (shown, shown && enabled)
}

type PreferenceResources<'w> = (
    ResMut<'w, Preferences>,
    ResMut<'w, Store>,
    ResMut<'w, UiScale>,
    ResMut<'w, bevy::audio::GlobalVolume>,
);

#[allow(clippy::too_many_arguments)] // Independent Bevy system parameters.
fn route(
    (keys, window): (
        Res<ButtonInput<KeyCode>>,
        Single<&Window, With<PrimaryWindow>>,
    ),
    (session, id, client, time): (
        Res<ClientSession>,
        Res<PlayerId>,
        Res<RenetClient>,
        Res<Time<Real>>,
    ),
    mut menu: ResMut<Menu>,
    mut editor: ResMut<loadout::Editor>,
    mut panel: ResMut<Panel>,
    mut input: ResMut<LocalInput>,
    widgets: WidgetQuery,
    mut pointer: MessageReader<PointerActivation>,
    mut activations: MessageWriter<Activation>,
    mut exit: MessageWriter<AppExit>,
    (mut preferences, mut store, mut ui_scale, mut volume): PreferenceResources,
    mut connection: Option<ResMut<Connection>>,
) {
    let old = menu.surface;
    let local = session.local(id.0);
    if local.is_some_and(|p| Some(p.participant.last_command) == menu.pending) {
        menu.pending = None;
    }
    let connected = connection_ready(&session, id.0, &client, time.elapsed_secs_f64());
    let playing = connected && session.playing(id.0);
    let arena_active =
        session.raid.phase == RaidPhase::Active && session.raid.mode != MatchMode::Raid;
    if !playing {
        menu.paused = false;
    }
    if !connected {
        editor.visible = false;
    }
    let lobby = connected && matches!(session.raid.phase, RaidPhase::Lobby | RaidPhase::Countdown);
    let countdown = session.raid.phase == RaidPhase::Countdown;
    let pending = menu.pending.is_some() || editor.pending.is_some();
    let editable = connected && !pending && editor.editable(&session, id.0);
    let settings_allowed = (playing && menu.paused) || (lobby && editable && !editor.visible);
    let escape = window.focused && keys.just_pressed(KeyCode::Escape) && !panel.consumed_escape;
    let bench_shortcut = menu.settings.is_none()
        && lobby
        && !countdown
        && window.focused
        && keys.just_pressed(KeyCode::KeyV);
    // Do not cover Cancel ready with the read-only bench during countdown.
    let mut consumed = countdown && editor.visible;
    if consumed {
        editor.visible = false;
        menu.lobby_focus = None;
    }
    if menu.settings.is_some() {
        if escape || !settings_allowed {
            menu.settings = None;
            consumed = true;
        }
    } else if escape && editor.visible {
        editor.visible = false;
        consumed = true;
    } else if bench_shortcut {
        if !editor.visible {
            menu.lobby_focus = widgets
                .iter()
                .find(|(_, _, action, _)| *action == Some(&Action::Bench))
                .map(|(entity, ..)| entity);
        }
        editor.visible = !editor.visible;
        consumed = true;
    } else if escape && playing {
        menu.paused = !menu.paused;
        consumed = true;
    }
    menu.surface = if !connected {
        Surface::Connection
    } else if menu.settings.is_some() {
        Surface::Settings
    } else if lobby && editor.visible {
        Surface::Bench
    } else if lobby {
        Surface::Lobby
    } else if session.raid.phase == RaidPhase::Debrief {
        Surface::Debrief
    } else if !playing {
        if arena_active {
            Surface::ArenaWaiting
        } else {
            Surface::Waiting
        }
    } else if menu.paused {
        if arena_active {
            Surface::ArenaPause
        } else {
            Surface::Pause
        }
    } else if panel.open {
        Surface::Inventory
    } else {
        Surface::Playing
    };
    if menu.paused && panel.open {
        *panel = Panel::default();
    }
    menu.transitioning_to_play = old != Surface::Playing && menu.surface == Surface::Playing;
    if old != menu.surface {
        menu.selected = if old == Surface::Bench && menu.surface == Surface::Lobby {
            menu.lobby_focus.take()
        } else if old == Surface::Settings
            && matches!(
                menu.surface,
                Surface::Lobby | Surface::Pause | Surface::ArenaPause
            )
        {
            menu.settings_focus.take()
        } else {
            None
        };
    }
    let mut candidates: Vec<_> = widgets
        .iter()
        .filter(|(_, _, action, bench)| {
            let (shown, enabled) = available(
                menu.surface,
                *action,
                *bench,
                editable,
                pending,
                countdown,
                editor.draft.as_ref(),
            );
            connection_action_visible(*action, connection.is_some(), client.is_disconnected())
                && (enabled
                    || (shown
                        && pending
                        && matches!(
                            action,
                            Some(Action::Ready | Action::Return | Action::LeaveArena)
                        )))
        })
        .collect();
    candidates.sort_unstable_by_key(|(_, order, ..)| order.0);
    let previous = menu.selected;
    let clicked = pointer
        .read()
        .filter(|entity| {
            window.focused
                && old == menu.surface
                && !consumed
                && candidates.iter().any(|entry| entry.0 == entity.entity)
        })
        .last();
    let index = candidates
        .iter()
        .position(|entry| Some(entry.0) == previous);
    let wants_previous = keys.just_pressed(KeyCode::ArrowUp)
        || keys.just_pressed(KeyCode::ArrowLeft)
        || keys.just_pressed(KeyCode::Tab)
            && (keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight));
    let wants_next = keys.just_pressed(KeyCode::ArrowDown)
        || keys.just_pressed(KeyCode::ArrowRight)
        || keys.just_pressed(KeyCode::Tab);
    let navigated = window.focused && !consumed && (wants_previous || wants_next);
    let len = candidates.len();
    let index = match index {
        Some(index) if navigated && wants_previous => (index + len - 1) % len,
        Some(index) if navigated => (index + 1) % len,
        Some(index) => index,
        None => 0,
    };
    menu.selected = clicked
        .map(|event| event.entity)
        .or_else(|| candidates.get(index).map(|entry| entry.0));
    let key_activate = window.focused
        && !consumed
        && !navigated
        && old == menu.surface
        && previous == menu.selected
        && (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space));
    if clicked.is_some_and(|event| event.focus_only) {
        return;
    }
    let Some(entity) = clicked
        .map(|event| event.entity)
        .or_else(|| key_activate.then_some(menu.selected).flatten())
    else {
        return;
    };
    let Ok((_, _, action, bench)) = widgets.get(entity) else {
        return;
    };
    if !available(
        menu.surface,
        action,
        bench,
        editable,
        pending,
        countdown,
        editor.draft.as_ref(),
    )
    .1
    {
        return;
    }
    match action.copied() {
        Some(Action::Ready | Action::Return | Action::LeaveArena) => {
            input.arena_leave = action == Some(&Action::LeaveArena);
            input.lobby_sequence = input.lobby_sequence.wrapping_add(1);
            menu.pending = Some(input.lobby_sequence);
            if lobby && local.is_some_and(|actor| !actor.participant.ready) {
                input.weapon = EquipmentSlot::Primary;
            }
        }
        Some(Action::Mode) => {
            input.mode = match input.mode {
                MatchMode::Raid => MatchMode::ArenaSolo,
                MatchMode::ArenaSolo => MatchMode::ArenaPvp,
                MatchMode::ArenaPvp => MatchMode::Raid,
            };
        }
        Some(Action::Bench) => {
            menu.lobby_focus = Some(entity);
            menu.selected = widgets
                .iter()
                .find(|(_, _, _, action)| *action == Some(&loadout::ui::Action::Close))
                .map(|(entity, ..)| entity);
            menu.surface = Surface::Bench;
            editor.visible = true;
        }
        Some(Action::Resume) => {
            menu.paused = false;
            // Gameplay remains blocked for the activation frame.
        }
        Some(Action::Quit) => {
            exit.write(AppExit::Success);
        }
        Some(Action::Reconnect | Action::Disconnect) => {
            if let Some(connection) = connection.as_mut() {
                connection.request = Some(if action == Some(&Action::Reconnect) {
                    Request::Connect
                } else {
                    Request::Disconnect
                });
            }
        }
        Some(Action::Settings) => {
            menu.settings_focus = Some(entity);
            menu.settings = Some(*preferences);
            menu.settings_status = if store.load_warning {
                "Invalid or unreadable file; using defaults."
            } else {
                ""
            };
            menu.surface = Surface::Settings;
            menu.selected = widgets
                .iter()
                .find(|(_, _, action, _)| *action == Some(&Action::FovLess))
                .map(|(entity, ..)| entity);
        }
        Some(Action::BackSettings) => {
            menu.settings = None;
            menu.surface = if lobby {
                Surface::Lobby
            } else if arena_active {
                Surface::ArenaPause
            } else {
                Surface::Pause
            };
            menu.selected = menu.settings_focus.take();
        }
        Some(action) if menu.surface == Surface::Settings => {
            let draft = menu
                .settings
                .as_mut()
                .expect("settings surface has a draft");
            match action {
                Action::FovLess => draft.fov = draft.fov.saturating_sub(5).max(20),
                Action::FovMore => draft.fov = (draft.fov + 5).min(160),
                Action::MouseLess => {
                    draft.sensitivity_percent = draft.sensitivity_percent.saturating_sub(5).max(25)
                }
                Action::MouseMore => {
                    draft.sensitivity_percent = (draft.sensitivity_percent + 5).min(300)
                }
                Action::ReloadKey => draft.reload = draft.reload.next(),
                Action::UiScaleCycle => {
                    draft.ui_scale_percent = if draft.ui_scale_percent == 100 {
                        125
                    } else {
                        100
                    };
                }
                Action::VolumeCycle => draft.volume_percent = (draft.volume_percent + 25) % 125,
                Action::Defaults => *draft = Preferences::default(),
                Action::Revert => *draft = *preferences,
                Action::ApplySettings => {
                    menu.settings_status = if let Err(error) = store.save(*draft) {
                        warn!("Could not save client preferences: {error}");
                        "Could not save; applied values unchanged."
                    } else {
                        if preferences.ui_scale_percent != draft.ui_scale_percent {
                            ui_scale.0 = draft.ui_scale();
                        }
                        if preferences.volume_percent != draft.volume_percent {
                            volume.volume = draft.volume();
                        }
                        *preferences = *draft;
                        "Applied and saved."
                    };
                    return;
                }
                _ => unreachable!("only settings actions are routed here"),
            }
            menu.settings_status = if *draft == *preferences {
                "No unapplied changes."
            } else {
                "Unapplied changes."
            };
        }
        Some(action) => change_kit(action, &mut input),
        None if bench == Some(&loadout::ui::Action::Close) => {
            editor.visible = false;
            menu.surface = Surface::Lobby;
            menu.selected = menu.lobby_focus.take();
        }
        None => {
            activations.write(Activation(entity));
        }
    }
}

fn change_kit(action: Action, input: &mut LocalInput) {
    match action {
        Action::Primary => {
            let primaries = common::catalog::catalog()
                .weapons()
                .filter(|weapon| weapon.class != WeaponKind::Pistol)
                .map(|weapon| weapon.definition)
                .collect::<Vec<_>>();
            if let Some(index) = primaries.iter().position(|id| *id == input.loadout.primary) {
                input.loadout.primary = primaries[(index + 1) % primaries.len()];
            }
            input.weapon = EquipmentSlot::Primary;
        }
        Action::Ammo => {
            input.loadout.ammo = match input.loadout.ammo {
                AmmoKind::Ball => AmmoKind::Ap,
                AmmoKind::Ap => AmmoKind::Ball,
            };
        }
        Action::Vest => {
            input.loadout.armor = match input.loadout.armor {
                ArmorKind::None => ArmorKind::Light,
                ArmorKind::Light => ArmorKind::Heavy,
                ArmorKind::Heavy => ArmorKind::None,
            };
        }
        Action::Helmet => input.loadout.helmet = !input.loadout.helmet,
        _ => unreachable!("only equipment actions reach kit changes"),
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Independent presentation queries.
fn present(
    (session, id, input): (Res<ClientSession>, Res<PlayerId>, Res<LocalInput>),
    client: Res<RenetClient>,
    (menu, editor): (Res<Menu>, Res<loadout::Editor>),
    mut root: Single<&mut Node, (With<Root>, Without<Action>)>,
    mut status: Single<&mut Text, With<Status>>,
    mut main: Query<(&Action, &Children, &mut Node), Without<Root>>,
    mut labels: Query<&mut Text, Without<Status>>,
    mut widgets: Query<
        (
            Entity,
            Option<&Action>,
            Option<&loadout::ui::Action>,
            &mut Outline,
            &mut BackgroundColor,
        ),
        With<NavOrder>,
    >,
    mut scroll: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition), With<BenchScroll>>,
    bounds: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut last_focus: Local<Option<Entity>>,
    connection: Option<Res<Connection>>,
    viewport: Single<&Window, With<PrimaryWindow>>,
    scale: Res<UiScale>,
) {
    // Reflow tall menus on short viewports. Grid auto-placement retains the
    // widget order, and the status spans both columns.
    let reflow = reflows(menu.surface, viewport.height() / scale.0);
    let was_reflow = root.display == Display::Grid;
    root.display = if matches!(
        menu.surface,
        Surface::Playing | Surface::Inventory | Surface::Bench
    ) {
        Display::None
    } else if reflow {
        Display::Grid
    } else {
        Display::Flex
    };
    if reflow != was_reflow {
        root.top = Val::Px(if reflow { 18.0 } else { 65.0 });
        root.right = if reflow { Val::Px(18.0) } else { Val::Auto };
        root.width = if reflow {
            Val::Auto
        } else {
            Val::Percent(42.0)
        };
        root.max_width = Val::Px(if reflow { 650.0 } else { 490.0 });
        root.column_gap = Val::Px(if reflow { 8.0 } else { 0.0 });
        root.grid_template_columns = if reflow {
            vec![RepeatedGridTrack::flex(2, 1.0)]
        } else {
            Vec::new()
        };
    }
    let actor = session.local(id.0);
    let participant = actor.map(|actor| &actor.participant);
    let ready = participant.is_some_and(|p| p.ready);
    let pending = menu.pending.is_some() || editor.pending.is_some();
    let editable = !pending && editor.editable(&session, id.0);
    let countdown = session.raid.phase == RaidPhase::Countdown;
    ***status = match menu.surface {
        Surface::Lobby => format!(
            "KIT PREPARATION | {}\n{}\nTab / arrows: select | Enter / Space: activate",
            if ready || !editable {
                mode_name(session.raid.mode)
            } else {
                mode_name(input.mode)
            },
            if menu.pending.is_some() {
                "Awaiting server confirmation".into()
            } else if editor.pending.is_some() {
                "Awaiting confirmed weapon kit".into()
            } else if session.raid.phase == RaidPhase::Countdown {
                format!(
                    "Deploying in {:.1}s | Kit locked",
                    session.raid.seconds_left
                )
            } else if ready {
                if session.raid.mode == MatchMode::ArenaPvp {
                    "Ready | Waiting for second player | Kit locked".into()
                } else {
                    "Ready | Kit locked".into()
                }
            } else if input.mode == MatchMode::ArenaSolo
                && session.players.iter().filter(|p| !p.bot).count() > 1
            {
                "Arena Solo requires only one connected player".into()
            } else {
                "Choose free equipment".into()
            }
        ),
        Surface::Pause | Surface::ArenaPause => {
            "LOCAL PAUSE\nServer and dangers continue.\nTab / arrows: select | Enter / Esc: resume"
                .into()
        }
        Surface::ArenaWaiting => {
            if menu.pending.is_some() {
                "ARENA | Leaving duel...".into()
            } else if participant.is_some_and(|p| p.status == RaidStatus::Dead) {
                let ticks = actor.map_or(0, |actor| actor.respawn_ticks);
                if ticks > 0 {
                    format!(
                        "ARENA | Eliminated\nRespawn in {:.1}s",
                        f32::from(ticks) / f32::from(SIMULATION_HZ)
                    )
                } else {
                    "ARENA | Eliminated\nWaiting for a clear spawn".into()
                }
            } else {
                "ARENA | Waiting for a combatant".into()
            }
        }
        Surface::Settings => format!(
            "LOCAL PREFERENCES\n{}\nTab / arrows: select | Enter / Space: activate",
            menu.settings_status
        ),
        Surface::Debrief | Surface::Waiting => {
            let p = participant.expect("connection_ready requires a local participant");
            let hint = if menu.pending.is_some() {
                "Awaiting lobby confirmation"
            } else if menu.surface == Surface::Debrief {
                "Tab / arrows: select | Enter / Space: activate"
            } else if p.status == common::raid::RaidStatus::Waiting {
                "Raid in progress. Waiting for next lobby."
            } else {
                "Waiting for other participants. No respawn in this raid."
            };
            let title = if menu.surface == Surface::Debrief {
                "DEBRIEF"
            } else {
                "RAID IN PROGRESS"
            };
            format!(
                "{title}\nResult: {:?} | Secured: {}\n{hint}",
                p.status, p.secured
            )
        }
        Surface::Connection if client.is_disconnected() => format!(
            "DISCONNECTED\n{}",
            connection
                .as_ref()
                .and_then(|connection| connection.notice.as_deref())
                .unwrap_or("Reconnect to start a fresh session.")
        ),
        Surface::Connection if client.is_connected() && participant.is_some() => {
            "CONNECTION STALE\nWaiting for authoritative state.".into()
        }
        Surface::Connection => "CONNECTING\nStart the server with cargo run --bin server.".into(),
        _ => String::new(),
    };
    for (action, children, mut node) in &mut main {
        node.display = if available(
            menu.surface,
            Some(action),
            None,
            editable,
            pending,
            countdown,
            None,
        )
        .0 && connection_action_visible(
            Some(action),
            connection.is_some(),
            client.is_disconnected(),
        ) {
            Display::Flex
        } else {
            Display::None
        };
        if let Ok(mut label) = labels.get_mut(children[0]) {
            let loadout = participant
                .filter(|p| p.ready)
                .map_or(&input.loadout, |p| &p.loadout);
            **label = match action {
                Action::Ready if ready => "Cancel ready".into(),
                Action::Ready => "Ready / deploy".into(),
                Action::Bench => "Weapon workbench".into(),
                Action::Mode => format!(
                    "Mode: {} (change)",
                    mode_name(if ready || !editable {
                        session.raid.mode
                    } else {
                        input.mode
                    })
                ),
                Action::Primary => format!("Primary: {}", loadout.primary.spec().name),
                Action::Ammo => format!("Ammo: {:?}", loadout.ammo),
                Action::Vest => format!("Vest: {:?}", loadout.armor),
                Action::Helmet => format!("Helmet: {}", if loadout.helmet { "On" } else { "Off" }),
                Action::Return => "Return everyone to lobby".into(),
                Action::Resume => "Resume".into(),
                Action::LeaveArena => "Leave Arena / return to lobby".into(),
                Action::Quit => "Quit client".into(),
                Action::Settings => "Preferences".into(),
                Action::FovLess => {
                    format!(
                        "- Vertical FOV: {} deg",
                        menu.settings.unwrap_or_default().fov
                    )
                }
                Action::FovMore => "+ Vertical FOV".into(),
                Action::MouseLess => format!(
                    "- Sensitivity: {}%",
                    menu.settings.unwrap_or_default().sensitivity_percent
                ),
                Action::MouseMore => "+ Sensitivity".into(),
                Action::ReloadKey => format!(
                    "Reload key: {} (cycle)",
                    menu.settings.unwrap_or_default().reload.label()
                ),
                Action::Defaults => "Use defaults in draft".into(),
                Action::UiScaleCycle => format!(
                    "UI scale: {}% (cycle)",
                    menu.settings.unwrap_or_default().ui_scale_percent
                ),
                Action::VolumeCycle => format!(
                    "New sounds: {}% (cycle)",
                    menu.settings.unwrap_or_default().volume_percent
                ),
                Action::Revert => "Revert unapplied changes".into(),
                Action::ApplySettings => "Apply and save".into(),
                Action::BackSettings => "Back (discard draft)".into(),
                Action::Reconnect => "Reconnect".into(),
                Action::Disconnect if menu.surface == Surface::Connection => {
                    "Cancel connection".into()
                }
                Action::Disconnect => "Disconnect from server".into(),
            };
        }
    }
    for (entity, action, bench, mut outline, mut color) in &mut widgets {
        let (shown, enabled) = available(
            menu.surface,
            action,
            bench,
            editable,
            pending,
            countdown,
            editor.draft.as_ref(),
        );
        let selected = shown && menu.selected == Some(entity);
        outline.color = if selected {
            Color::srgb(0.95, 0.83, 0.40)
        } else {
            Color::NONE
        };
        color.0 = if selected && enabled {
            Color::srgb(0.19, 0.32, 0.40)
        } else if enabled {
            Color::srgb(0.09, 0.16, 0.22)
        } else {
            Color::srgb(0.06, 0.07, 0.08)
        };
    }
    if *last_focus != menu.selected
        && menu.surface == Surface::Bench
        && let Some(entity) = menu.selected
        && let Ok((_, _, Some(action), ..)) = widgets.get(entity)
        && action.order() >= 3
        && let Ok((item, item_transform)) = bounds.get(entity)
        && let Ok((viewport, viewport_transform, mut offset)) = scroll.single_mut()
    {
        let (_, _, item_center) = item_transform.to_scale_angle_translation();
        let (_, _, view_center) = viewport_transform.to_scale_angle_translation();
        let delta = item_center.y - view_center.y;
        let margin = (viewport.size().y - item.size().y).max(0.0) / 2.0;
        offset.0.y += (delta - delta.clamp(-margin, margin)) * viewport.inverse_scale_factor();
    }
    *last_focus = menu.selected;
}

fn reflows(surface: Surface, logical_height: f32) -> bool {
    matches!(surface, Surface::Lobby | Surface::Settings) && logical_height < 570.0
}

fn mode_name(mode: MatchMode) -> &'static str {
    match mode {
        MatchMode::Raid => "Raid",
        MatchMode::ArenaSolo => "Arena Solo",
        MatchMode::ArenaPvp => "Arena PvP 1v1",
    }
}

fn connection_action_visible(action: Option<&Action>, installed: bool, disconnected: bool) -> bool {
    match action {
        Some(Action::Reconnect) => installed && disconnected,
        Some(Action::Disconnect) => installed && !disconnected,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        camera::NormalizedRenderTarget,
        input::mouse::AccumulatedMouseMotion,
        picking::{
            backend::HitData,
            events::Pointer,
            pointer::{Location, PointerId},
        },
        window::{CursorGrabMode, CursorOptions, WindowRef},
    };
    use common::ClientData;

    fn fixture() -> App {
        let mut app = App::new();
        let mut client = RenetClient::new(common::data::renet_config(), false);
        client.set_connected();
        let mut session = ClientSession::default();
        session.players.push(ClientData { id: 7, ..default() });
        app.add_message::<AppExit>()
            .insert_resource(session)
            .insert_resource(client)
            .insert_resource(PlayerId(7))
            .init_resource::<crate::sync::inventory::State>()
            .init_resource::<Assets<Font>>()
            .init_resource::<Time<Real>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .configure_sets(
                Update,
                (ClientSystems::Input, ClientSystems::Present).chain(),
            )
            .add_plugins((Plugin, crate::input::Plugin));
        loadout::setup_test_widgets(&mut app);
        app.world_mut().spawn((
            PrimaryWindow,
            Window {
                focused: true,
                ..default()
            },
            CursorOptions::default(),
        ));
        app.update();
        app
    }

    fn button<T: Component + PartialEq>(app: &mut App, action: T) -> Entity {
        let mut query = app.world_mut().query::<(Entity, &T)>();
        query
            .iter(app.world())
            .find(|(_, candidate)| **candidate == action)
            .unwrap()
            .0
    }

    fn click(app: &mut App, entity: Entity) {
        let mut query = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>();
        let window = query.single(app.world()).unwrap();
        app.world_mut().trigger(PointerClick {
            entity,
            pointer: Pointer::new(
                PointerId::Mouse,
                Location {
                    target: NormalizedRenderTarget::Window(
                        WindowRef::Entity(window).normalize(None).unwrap(),
                    ),
                    position: Vec2::ZERO,
                },
            ),
            button: PointerButton::Primary,
            hit: HitData::new(window, 0.0, None, None),
            duration: std::time::Duration::ZERO,
            count: 1,
        });
    }

    fn key(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(key);
        app.update();
    }

    #[test]
    fn compact_lobby_reflows_without_changing_pause_or_debrief() {
        let height_200 = 720.0 / 2.0;
        let height_125 = 720.0 / 1.25;
        assert!(reflows(Surface::Lobby, height_200));
        assert!(!reflows(Surface::Lobby, height_125));
        assert!(reflows(Surface::Settings, height_200));
        for surface in [Surface::Pause, Surface::ArenaPause, Surface::Debrief] {
            assert!(!reflows(surface, height_200));
        }
    }

    #[test]
    fn menu_preferences_draft_does_not_ready_and_apply_persists() {
        let mut app = fixture();
        let path =
            std::env::temp_dir().join(format!("tk-menu-prefs-{:016x}.json", rand::random::<u64>()));
        app.insert_resource(Store::from_path(Some(path.clone())).0);
        let settings = button(&mut app, Action::Settings);
        let fov_more = button(&mut app, Action::FovMore);
        let scale = button(&mut app, Action::UiScaleCycle);
        let volume = button(&mut app, Action::VolumeCycle);
        let apply = button(&mut app, Action::ApplySettings);
        let revert = button(&mut app, Action::Revert);
        click(&mut app, settings);
        app.update();
        assert_eq!(app.world().resource::<Menu>().surface, Surface::Settings);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.world().resource::<Menu>().settings.unwrap().fov, 65);
        assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 0);
        assert_eq!(app.world().resource::<Preferences>().fov, 70);
        click(&mut app, revert);
        app.update();
        assert_eq!(app.world().resource::<Menu>().settings.unwrap().fov, 70);
        click(&mut app, fov_more);
        app.update();
        click(&mut app, scale);
        app.update();
        assert_eq!(
            app.world()
                .resource::<Menu>()
                .settings
                .unwrap()
                .ui_scale_percent,
            125
        );
        assert_eq!(app.world().resource::<UiScale>().0, 1.0);
        click(&mut app, volume);
        app.update();
        assert!(
            (app.world()
                .resource::<bevy::audio::GlobalVolume>()
                .volume
                .to_linear()
                - 1.0)
                .abs()
                < f32::EPSILON
        );
        click(&mut app, apply);
        app.update();
        assert_eq!(app.world().resource::<Preferences>().fov, 75);
        assert_eq!(app.world().resource::<UiScale>().0, 1.25);
        let saved: Preferences = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.ui_scale_percent, 125);
        assert_eq!(saved.volume_percent, 0);
        assert!(
            app.world()
                .resource::<bevy::audio::GlobalVolume>()
                .volume
                .to_linear()
                .abs()
                < f32::EPSILON
        );
        assert!(path.exists());
        key(&mut app, KeyCode::Escape);
        assert_eq!(app.world().resource::<Menu>().selected, Some(settings));
        app.world_mut().resource_mut::<ClientSession>().players[0]
            .participant
            .ready = true;
        click(&mut app, settings);
        key(&mut app, KeyCode::KeyV); // The existing bench remains separate.
        key(&mut app, KeyCode::Escape);
        click(&mut app, settings);
        app.update();
        assert_eq!(app.world().resource::<Menu>().surface, Surface::Lobby);
        assert!(app.world().resource::<Menu>().settings.is_none());
        assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 0);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn menu_preferences_repair_clears_load_warning_only_after_successful_save() {
        use std::fs;

        let dir =
            std::env::temp_dir().join(format!("tk-menu-repair-{:016x}", rand::random::<u64>()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("preferences.json");
        let invalid = br#"{"fov":null}"#;
        fs::write(&path, invalid).unwrap();
        let (store, prefs) = Store::from_path(Some(path.clone()));
        assert_eq!(prefs, Preferences::default());
        let mut app = fixture();
        app.insert_resource(store).insert_resource(prefs);
        let settings = button(&mut app, Action::Settings);
        let fov_more = button(&mut app, Action::FovMore);
        let apply = button(&mut app, Action::ApplySettings);
        let back = button(&mut app, Action::BackSettings);
        let activate = |app: &mut App, entity| {
            click(app, entity);
            app.update();
        };
        let warning = "Invalid or unreadable file; using defaults.";

        activate(&mut app, settings);
        assert_eq!(app.world().resource::<Menu>().settings_status, warning);
        activate(&mut app, fov_more);

        // A directory at the destination makes the atomic rename fail even as root.
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        activate(&mut app, apply);
        assert_eq!(
            app.world().resource::<Menu>().settings_status,
            "Could not save; applied values unchanged."
        );
        assert_eq!(*app.world().resource::<Preferences>(), prefs);
        fs::remove_dir(&path).unwrap();
        fs::write(&path, invalid).unwrap();
        activate(&mut app, back);
        activate(&mut app, settings);
        assert_eq!(app.world().resource::<Menu>().settings_status, warning);

        activate(&mut app, fov_more);
        activate(&mut app, apply);
        assert_eq!(
            app.world().resource::<Menu>().settings_status,
            "Applied and saved."
        );
        let applied = *app.world().resource::<Preferences>();
        assert_eq!(applied.fov, 75);
        let saved: Preferences = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved, applied);
        activate(&mut app, back);
        activate(&mut app, settings);
        let menu = app.world().resource::<Menu>();
        assert_eq!(menu.settings, Some(applied));
        assert_eq!(menu.settings_status, "");
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    fn focus(app: &App, expected: Entity) {
        assert_eq!(app.world().resource::<Menu>().selected, Some(expected));
        assert_ne!(
            app.world().get::<Outline>(expected).unwrap().color,
            Color::NONE
        );
    }

    #[test]
    fn menu_keyboard_workbench_back_restores_focus_without_ready() {
        let mut app = fixture();
        let ready = button(&mut app, Action::Ready);
        let bench = button(&mut app, Action::Bench);
        let back = button(&mut app, loadout::ui::Action::Close);
        focus(&app, ready);
        key(&mut app, KeyCode::Tab);
        focus(&app, bench);
        key(&mut app, KeyCode::Enter);
        assert!(app.world().resource::<loadout::Editor>().visible);
        focus(&app, back);
        key(&mut app, KeyCode::Space);
        focus(&app, bench);
        assert!(!app.world().resource::<loadout::Editor>().visible);
        assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 0);
        key(&mut app, KeyCode::ArrowUp);
        focus(&app, ready);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 1);
    }

    #[test]
    fn menu_socket_marker_focuses_row_without_cycling_then_enter_edits() {
        let mut app = fixture();
        let kit = common::catalog::catalog()
            .configuration(common::WeaponId::M4A1)
            .unwrap();
        let bench = button(&mut app, Action::Bench);
        let part = button(&mut app, loadout::ui::Action::Part(0));
        let marker = app
            .world_mut()
            .query::<(Entity, &BenchFocusTarget)>()
            .iter(app.world())
            .find(|(_, target)| target.0 == part)
            .unwrap()
            .0;
        click(&mut app, bench);
        app.update();
        app.world_mut().resource_mut::<loadout::Editor>().draft =
            Some(common::catalog::WeaponKit {
                weapon: kit.clone(),
                reserves: vec![],
            });
        click(&mut app, marker);
        key(&mut app, KeyCode::Enter);
        focus(&app, part);
        assert_eq!(
            app.world()
                .resource::<loadout::Editor>()
                .draft
                .as_ref()
                .unwrap()
                .weapon,
            kit
        );
        key(&mut app, KeyCode::Enter);
        assert_ne!(
            app.world()
                .resource::<loadout::Editor>()
                .draft
                .as_ref()
                .unwrap()
                .weapon,
            kit
        );
    }

    #[test]
    fn menu_pointer_and_key_coalesce_and_observe_ready_lock_and_next_raid() {
        let mut app = fixture();
        let bench = button(&mut app, Action::Bench);
        let back = button(&mut app, loadout::ui::Action::Close);
        let primary = button(&mut app, Action::Primary);
        let ready = button(&mut app, Action::Ready);
        click(&mut app, bench);
        app.update();
        app.update();
        click(&mut app, back);
        app.update();
        focus(&app, bench);
        click(&mut app, primary);
        app.update();
        let kit = app.world().resource::<LocalInput>().loadout;
        assert_eq!(kit.primary, common::WeaponId::MP5);

        click(&mut app, ready);
        key(&mut app, KeyCode::Enter);
        click(&mut app, ready);
        key(&mut app, KeyCode::Enter);
        click(&mut app, primary);
        key(&mut app, KeyCode::KeyG);
        let input = app.world().resource::<LocalInput>();
        assert_eq!(input.lobby_sequence, 1);
        assert_eq!(input.loadout, kit);
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.phase = RaidPhase::Countdown;
            session.players[0].participant.last_command = 1;
            session.players[0].participant.ready = true;
        }
        click(&mut app, primary);
        key(&mut app, KeyCode::KeyB);
        assert_eq!(app.world().resource::<LocalInput>().loadout, kit);
        click(&mut app, ready);
        app.update();
        assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 2);

        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.phase = RaidPhase::Debrief;
            session.players[0].participant.last_command = 2;
        }
        key(&mut app, KeyCode::Enter); // transition itself cannot retarget an activation
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 3);
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.phase = RaidPhase::Lobby;
            session.players[0].participant.last_command = 3;
            session.players[0].participant.return_to_lobby();
        }
        app.update();
        click(&mut app, ready);
        app.update();
        assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 4);
    }

    #[test]
    fn menu_countdown_keeps_cancel_ready_accessible_from_workbench() {
        let mut app = fixture();
        let ready = button(&mut app, Action::Ready);
        let bench = button(&mut app, Action::Bench);
        click(&mut app, ready);
        app.update();
        assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 1);
        key(&mut app, KeyCode::KeyV);
        assert!(app.world().resource::<loadout::Editor>().visible);
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.phase = RaidPhase::Countdown;
            session.players[0].participant.last_command = 1;
            session.players[0].participant.ready = true;
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        assert_eq!(app.world().resource::<Menu>().surface, Surface::Lobby);
        focus(&app, ready);
        for use_shortcut in [false, true] {
            if use_shortcut {
                key(&mut app, KeyCode::KeyV);
            } else {
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .reset_all();
                click(&mut app, bench);
                app.update();
            }
            assert_eq!(app.world().resource::<Menu>().surface, Surface::Lobby);
            assert!(!app.world().resource::<loadout::Editor>().visible);
            focus(&app, ready);
        }
        assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 1);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 2);
        assert!(
            app.world()
                .resource::<ClientSession>()
                .local(7)
                .unwrap()
                .participant
                .ready
        );
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.phase = RaidPhase::Lobby;
            session.players[0].participant.last_command = 2;
            session.players[0].participant.ready = false;
        }
        key(&mut app, KeyCode::KeyV);
        assert_eq!(app.world().resource::<Menu>().surface, Surface::Bench);
    }

    #[test]
    fn menu_pause_consumes_combat_clicks_and_escape_respects_inventory_first() {
        let mut app = fixture();
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.phase = RaidPhase::Active;
            session.players[0].participant.begin();
        }
        app.update();
        key(&mut app, KeyCode::Tab);
        assert!(app.world().resource::<Panel>().open);
        key(&mut app, KeyCode::Escape);
        assert!(!app.world().resource::<Panel>().open);
        assert!(!app.world().resource::<Menu>().paused());
        key(&mut app, KeyCode::Escape);
        assert!(app.world().resource::<Menu>().paused());
        let mut cursor = app.world_mut().query::<&CursorOptions>();
        assert_eq!(
            cursor.single(app.world()).unwrap().grab_mode,
            CursorGrabMode::None
        );
        let resume = button(&mut app, Action::Resume);
        let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        buttons.press(MouseButton::Left);
        buttons.press(MouseButton::Right);
        click(&mut app, resume);
        key(&mut app, KeyCode::Enter);
        app.update();
        let input = app.world().resource::<LocalInput>();
        assert!(!input.fire && !input.aim);
        assert_eq!((input.fire_pressed_sequence, input.lobby_sequence), (0, 0));
        assert_eq!(
            cursor.single(app.world()).unwrap().grab_mode,
            CursorGrabMode::Locked
        );
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset_all();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert!(app.world().resource::<LocalInput>().fire);
    }

    #[test]
    fn menu_paused_preferences_keep_applied_values_when_save_fails() {
        let mut app = fixture();
        let applied = *app.world().resource::<Preferences>();
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.phase = RaidPhase::Active;
            session.players[0].participant.begin();
        }
        // The fixture uses a default Store without a writable path.
        let settings = button(&mut app, Action::Settings);
        let scale = button(&mut app, Action::UiScaleCycle);
        let volume = button(&mut app, Action::VolumeCycle);
        let apply = button(&mut app, Action::ApplySettings);
        key(&mut app, KeyCode::Escape);
        assert_eq!(app.world().resource::<Menu>().surface, Surface::Pause);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        for control in [settings, scale, volume, apply] {
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
            click(&mut app, control);
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .reset_all();
        }
        let menu = app.world().resource::<Menu>();
        assert_eq!(menu.surface, Surface::Settings);
        assert_eq!(
            menu.settings_status,
            "Could not save; applied values unchanged."
        );
        let draft = menu.settings.unwrap();
        assert_eq!((draft.ui_scale_percent, draft.volume_percent), (125, 0));
        assert_eq!(*app.world().resource::<Preferences>(), applied);
        assert_eq!(app.world().resource::<UiScale>().0, applied.ui_scale());
        assert_eq!(
            app.world().resource::<bevy::audio::GlobalVolume>().volume,
            applied.volume()
        );
        key(&mut app, KeyCode::Escape);
        let menu = app.world().resource::<Menu>();
        assert_eq!(menu.surface, Surface::Pause);
        assert!(menu.settings.is_none());
        focus(&app, settings);
        let input = app.world().resource::<LocalInput>();
        assert!(!input.fire && !input.aim);
        assert_eq!((input.fire_pressed_sequence, input.lobby_sequence), (0, 0));
    }

    #[test]
    fn pause_preferences_stale_recovery_waits_for_window_focus_before_capturing_cursor() {
        let mut app = fixture();
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.phase = RaidPhase::Active;
            session.players[0].participant.begin();
        }
        let mut window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>();
        let window = window.single(app.world()).unwrap();
        let assert_cursor = |app: &App, mode, visible| {
            let cursor = app.world().get::<CursorOptions>(window).unwrap();
            assert_eq!((cursor.grab_mode, cursor.visible), (mode, visible));
        };
        let settings = button(&mut app, Action::Settings);
        let more = button(&mut app, Action::FovMore);
        let apply = button(&mut app, Action::ApplySettings);
        let quit = button(&mut app, Action::Quit);
        let applied = *app.world().resource::<Preferences>();
        key(&mut app, KeyCode::Escape);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        click(&mut app, settings);
        app.update();
        click(&mut app, more);
        app.update();
        focus(&app, more);
        assert_eq!(app.world().resource::<Menu>().settings.unwrap().fov, 75);
        assert_cursor(&app, CursorGrabMode::None, true);

        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        click(&mut app, apply);
        key(&mut app, KeyCode::Escape);
        let menu = app.world().resource::<Menu>();
        assert_eq!(menu.surface, Surface::Settings);
        assert_eq!(menu.settings.unwrap().fov, 75);
        focus(&app, more);
        assert_eq!(*app.world().resource::<Preferences>(), applied);
        assert_cursor(&app, CursorGrabMode::None, true);

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs(2));
        app.update();
        let menu = app.world().resource::<Menu>();
        assert_eq!(menu.surface, Surface::Connection);
        assert!(menu.settings.is_none() && !menu.paused());
        focus(&app, quit);
        assert_eq!(app.world().get::<Outline>(more).unwrap().color, Color::NONE);
        assert_cursor(&app, CursorGrabMode::None, true);

        app.world_mut()
            .resource_mut::<ClientSession>()
            .last_received = 2.0;
        app.update();
        let menu = app.world().resource::<Menu>();
        assert_eq!(menu.surface, Surface::Playing);
        assert!(menu.settings.is_none() && menu.focused_widget().is_none());
        assert_eq!(app.world().get::<Outline>(quit).unwrap().color, Color::NONE);
        assert_eq!(*app.world().resource::<Preferences>(), applied);
        assert_cursor(&app, CursorGrabMode::None, true);

        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        assert_cursor(&app, CursorGrabMode::Locked, false);
    }

    #[test]
    fn arena_preferences_return_focus_to_the_invoking_control() {
        for escape in [false, true] {
            let mut app = fixture();
            {
                let mut session = app.world_mut().resource_mut::<ClientSession>();
                session.raid.phase = RaidPhase::Active;
                session.raid.mode = MatchMode::ArenaSolo;
                session.players[0].participant.begin();
            }
            let settings = button(&mut app, Action::Settings);
            let back = button(&mut app, Action::BackSettings);
            key(&mut app, KeyCode::Escape);
            assert_eq!(app.world().resource::<Menu>().surface, Surface::ArenaPause);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            click(&mut app, settings);
            app.update();
            assert_eq!(app.world().resource::<Menu>().surface, Surface::Settings);
            if escape {
                key(&mut app, KeyCode::Escape);
            } else {
                click(&mut app, back);
                app.update();
            }
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            for _ in 0..2 {
                let menu = app.world().resource::<Menu>();
                assert_eq!(menu.surface, Surface::ArenaPause);
                assert!(menu.settings.is_none());
                focus(&app, settings);
                assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 0);
                app.update();
            }
        }
    }

    #[test]
    fn menu_resume_space_does_not_jump_until_released_and_pressed_again() {
        let mut app = fixture();
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.phase = RaidPhase::Active;
            session.players[0].participant.begin();
        }
        key(&mut app, KeyCode::Escape);
        let resume = button(&mut app, Action::Resume);
        focus(&app, resume);
        key(&mut app, KeyCode::Space);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear_just_pressed(KeyCode::Space);
        for _ in 0..4 {
            app.update();
            assert!(!app.world().resource::<LocalInput>().jump);
        }
        assert_eq!(app.world().resource::<Menu>().surface, Surface::Playing);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Space);
        app.update();
        key(&mut app, KeyCode::Space);
        assert!(app.world().resource::<LocalInput>().jump);
    }

    #[test]
    fn menu_arena_mode_ready_then_leave_while_waiting_and_ready_again() {
        let mut app = fixture();
        let mode = button(&mut app, Action::Mode);
        let ready = button(&mut app, Action::Ready);
        let leave = button(&mut app, Action::LeaveArena);
        click(&mut app, mode);
        app.update();
        assert_eq!(
            app.world().resource::<LocalInput>().mode,
            MatchMode::ArenaSolo
        );
        assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 0);
        click(&mut app, ready);
        key(&mut app, KeyCode::Enter);
        assert!(!app.world().resource::<LocalInput>().arena_leave);
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.phase = RaidPhase::Active;
            session.raid.mode = MatchMode::ArenaSolo;
            session.players[0].respawn_ticks = 320;
            session.players[0].participant.last_command = 1;
            session.players[0].participant.fail(RaidStatus::Dead);
        }
        app.update();
        focus(&app, leave);
        key(&mut app, KeyCode::Enter);
        assert!(app.world().resource::<LocalInput>().arena_leave);
        assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 2);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 2);
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.phase = RaidPhase::Lobby;
            session.players[0].participant.last_command = 2;
            session.players[0].participant.return_to_lobby();
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        click(&mut app, ready);
        app.update();
        assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 3);
        assert!(!app.world().resource::<LocalInput>().arena_leave);
    }

    #[test]
    fn menu_stale_state_cannot_ready_or_replay_hidden_clicks() {
        let mut app = fixture();
        let ready = button(&mut app, Action::Ready);
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs(3));
        click(&mut app, ready);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.world().resource::<Menu>().surface, Surface::Connection);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ClientSession>()
            .last_received = 3.0;
        app.update();
        app.update();
        focus(&app, ready);
        assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 0);
    }
}
