//! Native panel acceptance goes through the real widget actions and reliable
//! commands. It never seeds the server fixture or edits the accepted cache.
use super::*;
use crate::loadout::{
    Editor,
    preview::{Preview, PreviewCamera, View},
    ui::Action,
};
use bevy::{
    camera::RenderTarget,
    picking::pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput},
    ui::UiGlobalTransform,
    window::{PrimaryWindow, WindowRef},
};

pub(super) fn enabled() -> bool {
    std::env::var_os("TK_PLAYTEST_CONFIGURATOR").is_some()
}

#[derive(Default)]
pub(super) struct Sequence {
    actions: Vec<Action>,
    index: usize,
    next: f64,
    captured: bool,
    review: Review,
}

#[derive(Default)]
struct Review {
    stage: u8,
    pointer_step: usize,
    baseline: Option<((u64, u32), Transform)>,
}

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct ReviewViews<'w, 's> {
    preview: Res<'w, Preview>,
    menu: Res<'w, crate::menu::Menu>,
    camera: Query<'w, 's, &'static Transform, With<PreviewCamera>>,
    markers: Query<
        'w,
        's,
        (
            &'static crate::menu::BenchFocusTarget,
            &'static Node,
            &'static ComputedNode,
            &'static UiGlobalTransform,
            &'static BackgroundColor,
        ),
    >,
    window: Query<'w, 's, (Entity, &'static Window), With<PrimaryWindow>>,
}

fn review_plan(actions: &[Action]) -> Vec<Action> {
    actions
        .iter()
        .copied()
        .chain([Action::Reset])
        .chain(actions.iter().copied())
        .chain([Action::Apply])
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn drive(
    mut commands: Commands,
    (time, session, id): (Res<Time<Real>>, Res<ClientSession>, Res<PlayerId>),
    mut driver: ResMut<Driver>,
    mut editor: ResMut<Editor>,
    buttons: Query<(Entity, &Action)>,
    mut activations: MessageWriter<crate::menu::Activation>,
    mut pointer: MessageWriter<PointerInput>,
    mut sequence: Local<Sequence>,
    views: ReviewViews,
) {
    if !enabled() || driver.step != Step::Equip || driver.panel_configured {
        return;
    }
    let now = time.elapsed_secs_f64();
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
    let review = !peer && std::env::var_os("TK_PLAYTEST_BENCH_DRAFT").is_some();
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
        if review {
            sequence.actions = review_plan(&sequence.actions);
        } else {
            sequence.actions.push(Action::Apply);
        }
    }
    if review && sequence.review.stage < 4 {
        let dry_count = (sequence.actions.len() - 2) / 2;
        let next_stage = sequence.review.stage;
        if next_stage <= 1
            || (next_stage == 2 && sequence.index == dry_count)
            || (next_stage == 3 && sequence.index == dry_count + 1)
        {
            let Some(accepted) = session
                .configurations
                .cache
                .latest(id.0, EquipmentSlot::Primary)
            else {
                return;
            };
            let handle = (accepted.packet.epoch, accepted.packet.handle);
            let kit = accepted.packet.kit(common::catalog::catalog()).unwrap();
            let button = buttons
                .iter()
                .find(|(_, action)| **action == Action::Part(0))
                .unwrap()
                .0;
            let Some((_, _, _, transform, tint)) =
                views.markers.iter().find(|(target, node, computed, ..)| {
                    target.0 == button
                        && node.display != Display::None
                        && computed.size().min_element() > 0.0
                })
            else {
                return;
            };
            if next_stage == 0 {
                assert_eq!(editor.draft.as_ref(), Some(&kit));
                let (entity, window) = views.window.single().unwrap();
                let location = Location {
                    target: RenderTarget::Window(WindowRef::Entity(entity))
                        .normalize(Some(entity))
                        .unwrap(),
                    position: (*transform * Vec2::ZERO) / window.scale_factor(),
                };
                let actions = [
                    PointerAction::Move { delta: Vec2::ZERO },
                    PointerAction::Press(PointerButton::Primary),
                    PointerAction::Release(PointerButton::Primary),
                ];
                pointer.write(PointerInput::new(
                    PointerId::Mouse,
                    location,
                    actions[sequence.review.pointer_step],
                ));
                sequence.review.pointer_step += 1;
                if sequence.review.pointer_step == actions.len() {
                    sequence.review.stage = 1;
                    sequence.next = now + 0.18;
                }
                return;
            }
            assert_eq!(views.menu.focused_widget(), Some(button));
            assert_eq!(tint.0, Color::srgb(0.95, 0.83, 0.40));
            assert!(editor.pending.is_none());
            let camera = *views.camera.single().unwrap();
            let (name, view) = match next_stage {
                1 => {
                    sequence.review.baseline = Some((handle, camera));
                    ("confirmed", View::Confirmed)
                }
                2 => ("draft", View::Draft),
                _ => ("discarded", View::Confirmed),
            };
            assert_eq!(sequence.review.baseline, Some((handle, camera)));
            assert_eq!(views.preview.view.as_ref(), Some(&view));
            assert_eq!(editor.draft.as_ref() == Some(&kit), view == View::Confirmed);
            capture(&mut commands, &format!("bench-{name}"));
            info!(
                "BENCH REVIEW {name} handle={} marker=0 focused=true",
                handle.1
            );
            sequence.review.stage += 1;
            sequence.next = now + 0.3;
            return;
        }
    }
    if let Some(action) = sequence.actions.get(sequence.index) {
        let (entity, _) = buttons
            .iter()
            .find(|(_, candidate)| *candidate == action)
            .expect("real panel button");
        activations.write(crate::menu::Activation(entity));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_draft_runs_the_same_edits_after_discard_and_applies_once() {
        let edits = [Action::Part(0), Action::Reserve(0)];
        let plan = review_plan(&edits);
        assert!(
            plan == [
                Action::Part(0),
                Action::Reserve(0),
                Action::Reset,
                Action::Part(0),
                Action::Reserve(0),
                Action::Apply,
            ]
        );
    }
}
