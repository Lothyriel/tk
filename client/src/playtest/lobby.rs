//! Native panel acceptance goes through the real widget actions and reliable
//! commands. It never seeds the server fixture or edits the accepted cache.
use super::*;
use crate::loadout::{Editor, ui::Action};

pub(super) fn enabled() -> bool {
    std::env::var_os("TK_PLAYTEST_CONFIGURATOR").is_some()
}

#[derive(Default)]
pub(super) struct Sequence {
    actions: Vec<Action>,
    index: usize,
    pressed: Option<Entity>,
    next: f64,
    captured: bool,
}

pub(super) fn drive(
    mut commands: Commands,
    (time, session, id): (Res<Time<Real>>, Res<ClientSession>, Res<PlayerId>),
    mut driver: ResMut<Driver>,
    mut editor: ResMut<Editor>,
    buttons: Query<(Entity, &Action)>,
    mut sequence: Local<Sequence>,
) {
    if !enabled() || driver.step != Step::Equip || driver.panel_configured {
        return;
    }
    let now = time.elapsed_secs_f64();
    if let Some(entity) = sequence.pressed.take() {
        commands.entity(entity).remove::<bevy::ui::Pressed>();
        return;
    }
    let Some(local) = session.local(id.0) else {
        return;
    };
    if local.participant.loadout.primary != WeaponId::M4A1
        || now - driver.since < 0.8
        || now < sequence.next
    {
        return;
    }
    let peer = std::env::var("TK_PLAYTEST_ROLE").is_ok_and(|role| role == "1");
    editor.visible = true;
    if sequence.actions.is_empty() {
        sequence.actions = if peer {
            let mut actions = vec![
                Action::Part(1),
                Action::Part(4),
                Action::Part(7),
                Action::Part(8),
                Action::Part(9),
                Action::Part(10),
            ];
            actions.extend([Action::Position(8); 3]);
            actions.extend([Action::Position(10); 9]);
            actions
        } else {
            vec![
                Action::Part(0),
                Action::Part(1),
                Action::Part(2),
                Action::Part(3),
                Action::Position(1),
                Action::Reserve(0),
                Action::Reserve(1),
                Action::Reserve(2),
            ]
        };
        sequence.actions.push(Action::Apply);
    }
    if let Some(action) = sequence.actions.get(sequence.index) {
        let (entity, _) = buttons
            .iter()
            .find(|(_, candidate)| *candidate == action)
            .expect("real panel button");
        commands.entity(entity).insert(bevy::ui::Pressed);
        sequence.pressed = Some(entity);
        sequence.index += 1;
        sequence.next = now + 0.18;
        return;
    }
    if editor.pending.is_some() {
        return;
    }
    let accepted = session
        .configurations
        .cache
        .latest(id.0, EquipmentSlot::Primary)
        .unwrap();
    assert_eq!(
        accepted.resolved.magazine_size,
        if peer { 30 } else { 20 },
        "{}",
        editor.status
    );
    let definitions: Vec<_> = accepted
        .packet
        .weapon
        .components
        .iter()
        .map(|part| part.definition.0)
        .collect();
    for expected in if peer {
        &[122, 128, 129, 132, 133, 134][..]
    } else {
        &[120, 122, 124, 126][..]
    } {
        assert!(
            definitions.contains(expected),
            "missing {expected}: {}",
            editor.status
        );
    }
    if !sequence.captured {
        capture(
            &mut commands,
            if peer {
                "configurator-suppressed"
            } else {
                "configurator-compact"
            },
        );
        sequence.captured = true;
        sequence.next = now + 0.6;
        info!(
            "LOBBY CONFIGURATOR PASS peer={peer} capacity={} definitions={definitions:?} magazines={:?}",
            accepted.resolved.magazine_size, accepted.packet.magazines
        );
        return;
    }
    editor.visible = false;
    driver.panel_configured = true;
}
