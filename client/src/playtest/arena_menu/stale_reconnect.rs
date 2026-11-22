//! Native connection widgets; synthetic Bevy keyboard events only.
use std::{fs, path::PathBuf};

use bevy::ui::UiSystems;
use bevy_renet2::{netcode::NetcodeClientTransport, prelude::RenetClient};

use super::*;

#[derive(Debug, Clone, Copy)]
enum Step {
    Lobby,
    Stale(u64),
    Cancelled(u64),
    Reconnected(u64),
}

#[derive(Resource)]
struct Route {
    directory: PathBuf,
    step: Step,
    release: bool,
}

pub(super) fn install(app: &mut App, directory: PathBuf) {
    assert!(directory.is_dir());
    for marker in ["lobby", "cancelled", "server-resumed"] {
        assert!(!directory.join(marker).exists(), "use a fresh directory");
    }
    app.insert_resource(Route {
        directory,
        step: Step::Lobby,
        release: false,
    })
    .add_systems(PostUpdate, observe.after(UiSystems::PostLayout));
}

type Network<'w> = (
    Res<'w, RenetClient>,
    Option<Res<'w, NetcodeClientTransport>>,
    Res<'w, ClientSession>,
    Res<'w, PlayerId>,
);

#[allow(clippy::too_many_arguments)]
fn observe(
    mut route: ResMut<Route>,
    time: Res<Time<Real>>,
    (client, transport, session, id): Network,
    (input, menu): (Res<LocalInput>, Res<Menu>),
    window: Single<(Entity, &Window, &CursorOptions), With<PrimaryWindow>>,
    widgets: Widgets,
    text: Query<&Text>,
    status: Query<(&Text, &ComputedNode)>,
    mut keyboard: MessageWriter<KeyboardInput>,
    mut exit: MessageWriter<AppExit>,
) {
    let (window_id, window, cursor) = *window;
    let now = time.elapsed_secs_f64();
    assert!(now < 15.0, "stale/reconnect timed out at {:?}", route.step);
    assert_eq!(input.lobby_sequence, 0, "unexpected Ready command");
    assert_eq!(input.fire_pressed_sequence, 0);
    assert!(!input.fire && !input.aim);
    if route.release {
        keyboard.write(key_event(window_id, KeyCode::Enter, ButtonState::Released));
        route.release = false;
        return;
    }
    if !window.focused {
        return;
    }
    let actor = session.local(id.0);
    if let Some(actor) = actor {
        assert!(!actor.participant.ready);
        assert_eq!(actor.participant.last_command, 0);
    }
    let says = |prefix: &str| {
        status
            .iter()
            .any(|(label, node)| label.starts_with(prefix) && node.size().min_element() > 0.0)
    };
    let focused = |label| {
        let (target, _, laid_out) =
            presented_button(&widgets, &text, label).expect("expected real menu widget");
        assert!(laid_out);
        assert_eq!(menu.focused_widget(), Some(target), "focus: {label}");
        assert!(
            widgets
                .get(target)
                .unwrap()
                .5
                .is_some_and(|outline| outline.color != Color::NONE),
            "missing focus outline: {label}"
        );
        assert!(menu.owns_input());
        assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    };
    let lobby = client.is_connected()
        && transport.is_some()
        && actor.is_some()
        && session.raid.phase == RaidPhase::Lobby
        && now - session.last_received < 2.0
        && says("KIT PREPARATION");
    let mut activate = false;
    route.step = match route.step {
        Step::Lobby if lobby => {
            focused("Ready / deploy");
            assert_ne!(id.0, 0);
            info!("STALE RECONNECT lobby id={} focus=Ready", id.0);
            fs::write(route.directory.join("lobby"), []).expect("notify server runner");
            Step::Stale(id.0)
        }
        Step::Stale(old) => {
            assert!(
                client.is_connected() && transport.is_some(),
                "transport disconnected before stale cancel"
            );
            assert_eq!(id.0, old);
            let age = now - session.last_received;
            if age < 2.0 {
                return;
            }
            assert!(actor.is_some());
            assert!(says("CONNECTION STALE\nWaiting for authoritative state."));
            assert!(presented_button(&widgets, &text, "Ready / deploy").is_none());
            assert!(presented_button(&widgets, &text, "Reconnect").is_none());
            focused("Cancel connection");
            info!(
                "STALE RECONNECT stale id={old} age={age:.3}s connected=true focus=Cancel connection"
            );
            activate = true;
            Step::Cancelled(old)
        }
        Step::Cancelled(old) => {
            // Transport cleanup resets Menu after Input; wait for the next
            // routing pass to assign the first disconnected-screen focus.
            if !client.is_disconnected() || !says("DISCONNECTED") || menu.focused_widget().is_none()
            {
                return;
            }
            assert!(transport.is_none());
            assert_eq!(id.0, 0);
            assert!(session.players.is_empty());
            assert!(says(
                "DISCONNECTED\nDisconnected. Reconnect to start a fresh session."
            ));
            assert!(presented_button(&widgets, &text, "Cancel connection").is_none());
            focused("Reconnect");
            let marker = route.directory.join("cancelled");
            if !marker.exists() {
                info!("STALE RECONNECT cancelled id=0 focus=Reconnect");
                fs::write(marker, []).expect("notify server runner");
            }
            if !route.directory.join("server-resumed").is_file() {
                return;
            }
            activate = true;
            Step::Reconnected(old)
        }
        Step::Reconnected(old) if lobby => {
            assert_ne!(id.0, 0);
            assert_ne!(id.0, old);
            focused("Ready / deploy");
            info!(
                "STALE RECONNECT PASS old={old} new={} focus=Ready sequence=0 (synthetic Bevy keyboard)",
                id.0
            );
            exit.write(AppExit::Success);
            return;
        }
        _ => return,
    };
    if activate {
        keyboard.write(key_event(window_id, KeyCode::Enter, ButtonState::Pressed));
        route.release = true;
    }
}
