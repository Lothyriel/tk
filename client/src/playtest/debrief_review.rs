//! Layout and keyboard activation of the real debrief after solo extraction.
use bevy::{
    input::{ButtonState, keyboard::KeyboardInput},
    prelude::*,
    ui::{UiGlobalTransform, UiSystems},
    ui_widgets::Button,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use common::{LocalInput, PlayerId, raid::RaidPhase};

use super::{ClientSession, arena_menu::key_event};
use crate::menu::Menu;

pub(super) fn enabled() -> bool {
    std::env::var_os("TK_DEBRIEF_REVIEW").is_some()
}

pub(super) fn install(app: &mut App) {
    assert_eq!(std::env::var("TK_PLAYTEST_CLIENTS").as_deref(), Ok("1"));
    assert!(std::env::var_os("TK_PLAYTEST_NO_CAPTURE").is_some());
    assert!(std::env::var_os("TK_PLAYTEST_VIDEO").is_none());
    assert_eq!(super::requested_viewport(), Some((1280, 720)));
    app.insert_resource(UiScale(2.0))
        .add_systems(PostUpdate, observe.after(UiSystems::PostLayout));
}

#[derive(Default)]
enum Review {
    #[default]
    Waiting,
    Pressed(u32),
    Released(u32),
    Done,
}

type Geometry<'w, 's> = Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform)>;

#[allow(clippy::too_many_arguments)] // Separate input and presentation queries.
fn observe(
    mut review: Local<Review>,
    (session, player, input, menu): (
        Res<ClientSession>,
        Res<PlayerId>,
        Res<LocalInput>,
        Res<Menu>,
    ),
    window: Single<(Entity, &Window, &CursorOptions), With<PrimaryWindow>>,
    scale: Res<UiScale>,
    labels: Query<(Entity, &Text, &ChildOf)>,
    buttons: Query<(Entity, &ChildOf, &Children, &Node, &Outline), With<Button>>,
    geometry: Geometry,
    mut keys: MessageWriter<KeyboardInput>,
) {
    let (window_entity, window, cursor) = *window;
    match *review {
        Review::Done => return,
        Review::Pressed(sequence) => {
            assert_eq!(input.lobby_sequence, sequence);
            keys.write(key_event(
                window_entity,
                KeyCode::Enter,
                ButtonState::Released,
            ));
            *review = Review::Released(sequence);
            return;
        }
        Review::Released(sequence) => {
            assert_eq!(input.lobby_sequence, sequence);
            if session.raid.phase == RaidPhase::Lobby {
                let participant = &session
                    .local(player.0)
                    .expect("returned participant")
                    .participant;
                assert_eq!(participant.last_command, sequence);
                assert!(!participant.ready);
                info!(
                    "DEBRIEF REVIEW PASS: synthetic Bevy Enter returned to unready lobby; acknowledged sequence={sequence}"
                );
                *review = Review::Done;
            }
            return;
        }
        Review::Waiting if session.raid.phase != RaidPhase::Debrief => return,
        Review::Waiting => {}
    }
    let Some((status, label, parent)) = labels
        .iter()
        .find(|(_, label, _)| label.starts_with("DEBRIEF\n"))
    else {
        return;
    };
    assert!(window.focused, "focus the native debrief window");
    assert!(menu.owns_input());
    assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    assert!(!input.fire && !input.aim);
    assert!(
        label.contains("Result: Extracted | Secured: 1"),
        "{label:?}"
    );
    assert_eq!(scale.0, 2.0);
    assert_eq!(
        (window.physical_width(), window.physical_height()),
        (1280, 720)
    );
    let bounds = |entity| {
        let (node, transform) = geometry.get(entity).expect("debrief geometry");
        assert!(node.size().min_element() > 0.0);
        let center = *transform * Vec2::ZERO;
        let half = node.size() * 0.5;
        (center - half, center + half)
    };
    let contains = |(min, max): (Vec2, Vec2), (inner_min, inner_max): (Vec2, Vec2)| {
        inner_min.cmpge(min).all() && inner_max.cmple(max).all()
    };
    let root = parent.parent();
    let panel = bounds(root);
    assert!(contains((Vec2::ZERO, Vec2::new(1280.0, 720.0)), panel));
    assert!(contains(panel, bounds(status)));
    info!(
        "DEBRIEF REVIEW panel={panel:?} status={:?} {label:?}",
        bounds(status)
    );
    let mut shown = Vec::new();
    let return_label = "Return everyone to lobby";
    for (entity, parent, children, node, outline) in &buttons {
        if parent.parent() != root || node.display == Display::None {
            continue;
        }
        let (label_entity, text, _) = labels.get(children[0]).expect("button label");
        let rect = bounds(entity);
        assert!(
            contains(panel, rect) && contains(rect, bounds(label_entity)),
            "{text:?} exceeds bounds"
        );
        let focused = menu.focused_widget() == Some(entity);
        assert_eq!(focused, text.as_str() == return_label);
        assert_eq!(outline.color != Color::NONE, focused);
        info!("DEBRIEF REVIEW {text:?} bounds={rect:?} focused={focused}");
        shown.push(text.as_str());
    }
    shown.sort_unstable();
    assert_eq!(
        shown,
        ["Disconnect from server", "Quit client", return_label]
    );
    let sequence = input.lobby_sequence;
    let actor = session.local(player.0).expect("debrief participant");
    assert_eq!(actor.participant.last_command, sequence);
    keys.write(key_event(
        window_entity,
        KeyCode::Enter,
        ButtonState::Pressed,
    ));
    *review = Review::Pressed(sequence.wrapping_add(1));
}
