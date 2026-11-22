//! Native, unready UDP lobby layout and focus checkpoint. Synthetic Bevy Tab only.
use bevy::ui::UiSystems;

use super::*;

pub(super) fn install(app: &mut App, value: &str) {
    let scale: f32 = value.parse().expect("numeric lobby review scale");
    assert!((1.0..=2.0).contains(&scale), "lobby review scale: 1–2");
    app.insert_resource(UiScale(scale))
        .add_systems(PostUpdate, observe.after(UiSystems::PostLayout));
}

type Geometry<'w, 's> = Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform)>;

// Player-facing order, independent of the router's NavOrder values.
const TAB_ORDER: [&str; 10] = [
    "Ready / deploy",
    "Weapon workbench",
    "Mode:",
    "Primary:",
    "Ammo:",
    "Vest:",
    "Helmet:",
    "Disconnect from server",
    "Quit client",
    "Preferences",
];

#[derive(Default)]
struct Traversal {
    presses: usize,
    release: bool,
}

#[allow(clippy::too_many_arguments)]
fn observe(
    time: Res<Time<Real>>,
    mut traversal: Local<Traversal>,
    scale: Res<UiScale>,
    (session, player, input, menu): (
        Res<ClientSession>,
        Res<PlayerId>,
        Res<LocalInput>,
        Res<Menu>,
    ),
    window: Single<(Entity, &Window), With<PrimaryWindow>>,
    widgets: Widgets,
    text: Query<(Entity, &Text, Option<&ChildOf>)>,
    parents: Query<&ChildOf>,
    nodes: Query<&Node>,
    geometry: Geometry,
    mut keys: MessageWriter<KeyboardInput>,
    mut exit: MessageWriter<AppExit>,
) {
    let (window_entity, window) = *window;
    assert!(
        time.elapsed_secs_f64() < 12.0,
        "lobby review timed out after {} Tab presses (window focused={})",
        traversal.presses,
        window.focused
    );
    assert_eq!(input.lobby_sequence, 0, "Tab must not send a lobby command");
    assert!(!input.fire && !input.aim);
    assert_eq!(input.fire_pressed_sequence, 0);
    let initial = traversal.presses == 0;
    let Some(actor) = session.local(player.0) else {
        assert!(initial, "lost the local actor during Tab traversal");
        return;
    };
    assert_eq!(actor.participant.last_command, 0);
    if session.raid.phase != RaidPhase::Lobby || actor.participant.ready {
        assert!(initial, "left the unready lobby during Tab traversal");
        return;
    }
    if !window.focused {
        assert!(initial, "window lost focus during Tab traversal");
        return;
    }
    let Some((status, label, Some(parent))) = text
        .iter()
        .find(|(_, label, _)| label.starts_with("KIT PREPARATION"))
    else {
        assert!(initial, "lobby status disappeared during Tab traversal");
        return;
    };
    let root = parent.parent();
    let size = Vec2::new(
        window.physical_width() as f32,
        window.physical_height() as f32,
    );
    let inside = |entity| {
        let (node, transform) = geometry.get(entity).expect("laid-out lobby entity");
        let center = *transform * Vec2::ZERO;
        let half = node.size() * 0.5;
        let min = center - half;
        let max = center + half;
        (
            node.size().min_element() > 0.0 && min.cmpge(Vec2::ZERO).all() && max.cmple(size).all(),
            min,
            max,
        )
    };
    let root_display = nodes.get(root).expect("lobby root style").display;
    let (root_inside, root_min, root_max) = inside(root);
    let (status_inside, status_min, status_max) = inside(status);
    if initial {
        info!(
            "LOBBY REVIEW viewport={size:?} scale={} display={root_display:?}",
            scale.0
        );
        info!(
            "LOBBY REVIEW root={root_min:?}..{root_max:?} status={status_min:?}..{status_max:?} {label:?}"
        );
    }
    assert!(label.contains("Choose free equipment"), "{label:?}");
    let mut controls = 0;
    let mut focused_label = None;
    let mut outlines = 0;
    let mut all_inside = root_inside && status_inside;
    for (entity, children, _, style, routed, outline) in &widgets {
        // The hidden workbench can still have locally displayed children.
        if !routed
            || style.display == Display::None
            || !parents
                .get(entity)
                .is_ok_and(|parent| parent.parent() == root)
        {
            continue;
        }
        let (label_entity, label, _) = text.get(children[0]).expect("lobby button label");
        let (button_inside, button_min, button_max) = inside(entity);
        let (label_inside, label_min, label_max) = inside(label_entity);
        let button_inside =
            button_inside && button_min.cmpge(root_min).all() && button_max.cmple(root_max).all();
        let label_inside =
            label_inside && label_min.cmpge(button_min).all() && label_max.cmple(button_max).all();
        let outlined = outline.is_some_and(|outline| outline.color != Color::NONE);
        let focused = menu.focused_widget() == Some(entity);
        if initial {
            info!(
                "LOBBY REVIEW {label:?} button={button_min:?}..{button_max:?} label={label_min:?}..{label_max:?} inside={} focused={focused}",
                button_inside && label_inside
            );
        }
        outlines += usize::from(outlined);
        if focused {
            assert!(outlined, "missing focus outline: {label:?}");
            focused_label = Some(label.as_str());
        }
        controls += 1;
        all_inside &= button_inside && label_inside;
    }
    assert_eq!(controls, TAB_ORDER.len());
    assert_eq!(outlines, 1, "expected one visible focus outline");
    assert!(all_inside, "lobby layout exceeds viewport");
    let focused_label = focused_label.expect("focus must be on a visible lobby control");
    let expected = TAB_ORDER[traversal.presses % TAB_ORDER.len()];
    assert!(
        focused_label.starts_with(expected),
        "after {} Tab presses: expected {expected:?}, focused {focused_label:?}",
        traversal.presses
    );
    if traversal.release {
        info!(
            "LOBBY REVIEW Tab {} focus={focused_label:?}",
            traversal.presses
        );
        keys.write(key_event(
            window_entity,
            KeyCode::Tab,
            ButtonState::Released,
        ));
        traversal.release = false;
    } else if traversal.presses == TAB_ORDER.len() {
        info!(
            "LOBBY REVIEW PASS: all {controls} controls inside viewport; synthetic Bevy Tab wrapped to Ready, no lobby command"
        );
        exit.write(AppExit::Success);
    } else {
        keys.write(key_event(window_entity, KeyCode::Tab, ButtonState::Pressed));
        traversal.presses += 1;
        traversal.release = true;
    }
}
