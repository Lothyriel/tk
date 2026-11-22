//! Static layout checkpoint in a native, unready UDP lobby. Sends no input.
use bevy::ui::UiSystems;

use super::*;

pub(super) fn install(app: &mut App, value: &str) {
    let scale: f32 = value.parse().expect("numeric lobby review scale");
    assert!((1.0..=2.0).contains(&scale), "lobby review scale: 1–2");
    app.insert_resource(UiScale(scale))
        .add_systems(PostUpdate, observe.after(UiSystems::PostLayout));
}

type Geometry<'w, 's> = Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform)>;

#[allow(clippy::too_many_arguments)]
fn observe(
    time: Res<Time<Real>>,
    scale: Res<UiScale>,
    (session, player, input, menu): (
        Res<ClientSession>,
        Res<PlayerId>,
        Res<LocalInput>,
        Res<Menu>,
    ),
    window: Single<&Window, With<PrimaryWindow>>,
    widgets: Widgets,
    text: Query<(Entity, &Text, Option<&ChildOf>)>,
    parents: Query<&ChildOf>,
    nodes: Query<&Node>,
    geometry: Geometry,
    mut exit: MessageWriter<AppExit>,
) {
    assert!(time.elapsed_secs_f64() < 12.0, "lobby review timed out");
    let Some(actor) = session.local(player.0) else {
        return;
    };
    if session.raid.phase != RaidPhase::Lobby || actor.participant.ready {
        return;
    }
    let Some((status, label, Some(parent))) = text
        .iter()
        .find(|(_, label, _)| label.starts_with("KIT PREPARATION"))
    else {
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
    info!(
        "LOBBY REVIEW viewport={size:?} scale={} display={root_display:?}",
        scale.0
    );
    let (root_inside, root_min, root_max) = inside(root);
    let (status_inside, status_min, status_max) = inside(status);
    info!(
        "LOBBY REVIEW root={root_min:?}..{root_max:?} status={status_min:?}..{status_max:?} {label:?}"
    );
    assert!(label.contains("Choose free equipment"), "{label:?}");
    assert_eq!(input.lobby_sequence, 0);
    assert_eq!(actor.participant.last_command, 0);
    let mut controls = 0;
    let mut ready_focused = false;
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
        let focused = menu.focused_widget() == Some(entity)
            && outline.is_some_and(|outline| outline.color != Color::NONE);
        info!(
            "LOBBY REVIEW {label:?} button={button_min:?}..{button_max:?} label={label_min:?}..{label_max:?} inside={} focused={focused}",
            button_inside && label_inside
        );
        controls += 1;
        all_inside &= button_inside && label_inside;
        ready_focused |= focused && label.as_str() == "Ready / deploy";
    }
    assert_eq!(controls, 10);
    assert!(
        ready_focused,
        "Ready must start with a visible focus outline"
    );
    assert!(all_inside, "lobby layout exceeds viewport");
    info!("LOBBY REVIEW PASS: status and all {controls} controls inside viewport");
    exit.write(AppExit::Success);
}
