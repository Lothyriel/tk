//! Native client probe, driven by synthetic keyboard events. Never writes an
//! inventory intent or replica.
use std::collections::{BTreeMap, VecDeque};

use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput, NativeKey},
    },
    prelude::*,
    window::PrimaryWindow,
};
use common::{
    LocalInput, PlayerId,
    catalog::{
        ItemInstanceId,
        item_grid::{Footprint, Placement, Rotation},
        item_inventory::{InventorySnapshot, ProjectedItem, SessionKey},
        item_manifest::planned_items,
    },
};

use crate::{
    ClientSystems,
    input::inventory_panel::Panel,
    sync::{ClientSession, inventory::State},
};

use super::inventory_frame_windows as frames;

pub(super) fn benchmark_enabled() -> bool {
    std::env::var_os("TK_PLAYTEST_GRID_BENCH").is_some()
}

pub(crate) struct Plugin {
    pub integrated: bool,
}

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Probe {
            integrated: self.integrated,
            benchmark: self.integrated && benchmark_enabled(),
            ..default()
        })
        .add_systems(Update, drive.after(ClientSystems::Present));
        if !self.integrated {
            app.add_systems(Startup, || {
                info!("GRID PROBE: collect a 1x2 item, then open the backpack grid with Tab");
            });
        }
    }
}

type Items = BTreeMap<ItemInstanceId, ProjectedItem>;
type Live<'w> = (
    Res<'w, ClientSession>,
    Res<'w, PlayerId>,
    Res<'w, State>,
    Res<'w, Panel>,
    Res<'w, LocalInput>,
    Res<'w, Time<Real>>,
);

#[derive(Debug)]
enum Step {
    Move,
    Split {
        source: ItemInstanceId,
        new_stack: ProjectedItem,
    },
    Merge,
}

impl Step {
    fn feedback(&self) -> &'static str {
        match self {
            Self::Move => "Move confirmed",
            Self::Split { .. } => "Split confirmed",
            Self::Merge => "Merge confirmed",
        }
    }
}

struct Check {
    step: Step,
    revision: u64,
    expected: Items,
}

#[derive(Resource, Default)]
pub(super) struct Probe {
    integrated: bool,
    requested: bool,
    opening: bool,
    closing: bool,
    run: Option<(SessionKey, f64, [u32; 2])>,
    keys: VecDeque<Vec<KeyCode>>,
    release: Vec<KeyCode>,
    check: Option<Check>,
    finished: bool,
    benchmark: bool,
    benchmark_run: Option<Benchmark>,
}

struct Benchmark {
    sampler: frames::Sampler,
    session: SessionKey,
    position: Vec3,
    rotation: Quat,
    previous: frames::Panel,
    cursor: Option<Vec2>,
    started: f64,
}

impl Probe {
    /// The loot driver must remain paused until Tab has closed the panel.
    pub(super) fn ready_to_resume(&mut self) -> bool {
        self.requested = true;
        self.finished
    }

    fn route(
        &mut self,
        grid: &InventorySnapshot,
        selected: ItemInstanceId,
        item: ItemInstanceId,
        target: Placement,
        step: Step,
        expected: Items,
    ) {
        use KeyCode::*;
        let index = |id| grid.items.iter().position(|(key, _)| *key == id).unwrap();
        let current = grid.items[index(item)].1.placement;
        let down = (index(item) + grid.items.len() - index(selected)) % grid.items.len();
        self.keys.extend((0..down).map(|_| vec![ArrowDown]));
        if matches!(step, Step::Split { .. }) {
            self.keys.extend([vec![ShiftLeft, Enter], vec![Enter]]);
        } else {
            self.keys.push_back(vec![Enter]);
        }
        if current.rotation != target.rotation && matches!(step, Step::Move) {
            self.keys.push_back(vec![KeyR]);
        }
        for (from, to, inc, dec) in [
            (current.x, target.x, ArrowRight, ArrowLeft),
            (current.y, target.y, ArrowDown, ArrowUp),
        ] {
            self.keys
                .extend((0..from.abs_diff(to)).map(|_| vec![if to > from { inc } else { dec }]));
        }
        self.keys.push_back(vec![Enter]);
        self.check = Some(Check {
            step,
            expected,
            revision: grid.version.revision + 1,
        });
    }
}

fn destination(
    grid: &InventorySnapshot,
    item: ItemInstanceId,
    footprint: Footprint,
    rotation: Rotation,
    splitting: bool,
) -> Option<Placement> {
    let size = Footprint::new(grid.size[0], grid.size[1])?;
    let source = grid.items.iter().find(|(id, _)| *id == item)?.1.placement;
    (0..grid.size[1])
        .flat_map(|y| (0..grid.size[0]).map(move |x| Placement { x, y, rotation }))
        .find(|&p| {
            (p.x, p.y) != (source.x, source.y)
                && p.area(footprint)
                    .validate(
                        size,
                        grid.items
                            .iter()
                            .filter(|(id, _)| splitting || *id != item)
                            .map(|(_, i)| {
                                i.placement
                                    .area(planned_items()[i.definition.as_str()].footprint)
                            }),
                    )
                    .is_ok()
        })
}

type ReviewControls<'w> = (
    MessageWriter<'w, AppExit>,
    Option<Res<'w, crate::input::inventory_panel::PointerRoute>>,
    Option<Res<'w, UiScale>>,
    Option<Res<'w, crate::input::inventory_panel::CloseAck>>,
);

fn drive(
    mut probe: ResMut<Probe>,
    (session, player, inventory, panel, input, time): Live,
    window: Single<(Entity, &Window), With<PrimaryWindow>>,
    mut keyboard: MessageWriter<KeyboardInput>,
    (mut exit, pointer, scale, close_ack): ReviewControls,
) {
    if probe.finished || (probe.integrated && !probe.requested) {
        return;
    }
    let (window_id, window) = *window;
    let emit = |key_code, state| KeyboardInput {
        key_code,
        window: window_id,
        state,
        repeat: false,
        text: None,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
    };
    let released = !probe.release.is_empty();
    if released {
        for key in probe.release.drain(..) {
            keyboard.write(emit(key, ButtonState::Released));
        }
        if !probe.benchmark {
            return;
        }
    }
    if probe.benchmark {
        let now = time.elapsed_secs_f64();
        let (Some(grid), Some(key), Some(actor)) = (
            inventory.owned_grid(session.raid.raid_id, player.0),
            inventory.grid_session(),
            session.local(player.0),
        ) else {
            panic!("grid benchmark lost its confirmed loot checkpoint");
        };
        let position = Vec3::from_array(actor.pos);
        let rotation = Quat::from(&actor.rot);
        let presented = if panel.open {
            frames::Panel::Open
        } else {
            frames::Panel::Closed
        };
        let cursor = window.cursor_position();
        let run = probe.benchmark_run.get_or_insert_with(|| Benchmark {
            sampler: frames::Sampler::new(grid.version.revision),
            session: key,
            position,
            rotation,
            previous: presented,
            cursor,
            started: now,
        });
        assert!(now - run.started < 175.0, "grid benchmark timed out");
        // Let the player's final approach settle before establishing the
        // baseline. Input is already paused at the confirmed pickup.
        if now - run.started < 1.5 {
            run.position = position;
            run.rotation = rotation;
            run.previous = presented;
            run.cursor = cursor;
            return;
        }
        let active = session.playing(player.0)
            && session.raid.seconds_left > 0.0
            && now - session.last_received < 2.0
            && run.session == key
            && actor.pose.mode == common::humanoid::PhysicalMode::Standing
            && actor.pose.grounded;
        let drift = position.distance(run.position);
        let turn = rotation.angle_between(run.rotation).abs();
        let ui_scale = scale.as_ref().map_or(1.0, |scale| scale.0);
        let idle = panel.grid_idle() && !input.fire && !input.aim;
        let cursor_stable = presented != run.previous || cursor == run.cursor;
        let checkpoint =
            active && drift < 0.03 && turn < 0.005 && ui_scale == 1.0 && idle && cursor_stable;
        run.sampler
            .observe(frames::Observation {
                panel: run.previous,
                checkpoint_valid: checkpoint,
                focused: window.focused,
                viewport: [window.physical_width(), window.physical_height()],
                revision: grid.version.revision,
                frame_ms: time.delta_secs_f64() * 1000.0,
            })
            .unwrap_or_else(|error| panic!(
                "grid benchmark invalidated: {error:?}; active={active} drift={drift:.4} turn={turn:.4} scale={ui_scale} idle={idle} cursor_stable={cursor_stable} panel={presented:?} pose={:?} grounded={}",
                actor.pose.mode, actor.pose.grounded
            ));
        run.previous = presented;
        run.cursor = cursor;
        if let Some(requested) = run.sampler.requested_panel() {
            if requested != presented && !released {
                keyboard.write(emit(KeyCode::Tab, ButtonState::Pressed));
                probe.release.push(KeyCode::Tab);
            }
        } else {
            for summary in run.sampler.summaries().expect("complete grid benchmark") {
                println!("{summary}");
            }
            probe.benchmark = false;
            probe.opening = panel.open;
        }
        return;
    }
    let diagnostic_move = close_ack.is_some()
        && probe.keys.is_empty()
        && probe
            .check
            .as_ref()
            .is_some_and(|check| matches!(check.step, Step::Move));
    if diagnostic_move && !close_ack.as_ref().is_some_and(|route| route.finished()) {
        assert!(
            probe
                .run
                .is_some_and(|(_, start, _)| time.elapsed_secs_f64() - start < 20.0),
            "GRID CLOSE ACK did not complete; inspect ordering trace"
        );
        return;
    }
    if probe.closing {
        assert!(
            !panel.open,
            "GRID PROBE closing Tab did not close the panel"
        );
        probe.finished = true;
        return;
    }
    if probe.integrated && !probe.opening && window.focused && session.playing(player.0) {
        assert!(!panel.open, "GRID PROBE expected a closed backpack");
        keyboard.write(emit(KeyCode::Tab, ButtonState::Pressed));
        probe.release.push(KeyCode::Tab);
        probe.opening = true;
        return;
    }
    let sequences = [input.fire_pressed_sequence, input.reload_sequence];
    if let Some((key, start, before)) = probe.run {
        assert!(
            time.elapsed_secs_f64() - start < 20.0,
            "GRID PROBE timed out waiting for {:?}",
            probe.check.as_ref().map(|c| (&c.step, c.revision))
        );
        assert!(
            window.focused
                && panel.open
                && panel.inspection.is_none()
                && session.playing(player.0)
                && inventory.grid_session() == Some(key),
            "GRID PROBE lost focus, panel, or live inventory session"
        );
        assert_eq!(sequences, before, "grid keys leaked into fire or reload");
        assert!(!input.fire && !input.aim, "grid leaked combat input");
    }
    if !window.focused || !panel.open || panel.inspection.is_some() || !session.playing(player.0) {
        return;
    }
    if pointer.is_some_and(|route| !route.finished()) {
        return;
    }
    let (Some(grid), Some(key), Some(selected)) = (
        inventory.owned_grid(session.raid.raid_id, player.0),
        inventory.grid_session(),
        panel.selected,
    ) else {
        return;
    };
    if let Some(keys) = probe.keys.pop_front() {
        assert_eq!(
            grid.version.revision + 1,
            probe.check.as_ref().unwrap().revision,
            "grid changed while preparing the operation"
        );
        for &key in &keys {
            keyboard.write(emit(key, ButtonState::Pressed));
        }
        probe.release = keys;
        return;
    }
    let mut actual: Items = grid.items.iter().cloned().collect();
    if let Some(check) = probe.check.take() {
        if grid.version.revision < check.revision
            || (!diagnostic_move && panel.grid_feedback() != Some(check.step.feedback()))
        {
            probe.check = Some(check);
            return;
        }
        assert_eq!(
            grid.version.revision, check.revision,
            "unexpected inventory revision"
        );
        info!(
            "GRID PROBE {:?} confirmed at revision {}",
            check.step, check.revision
        );
        match check.step {
            Step::Move => {
                assert_eq!(actual, check.expected);
                if let Some((&id, bandage)) = actual
                    .iter()
                    .find(|(_, i)| i.definition == "med.bandage" && i.quantity >= 2)
                {
                    let footprint = planned_items()["med.bandage"].footprint;
                    let target = destination(grid, id, footprint, bandage.placement.rotation, true)
                        .expect("GRID PROBE needs a free cell for the bandage split");
                    let new_stack = ProjectedItem {
                        placement: target,
                        quantity: 1,
                        ..bandage.clone()
                    };
                    actual.get_mut(&id).unwrap().quantity -= 1;
                    probe.route(
                        grid,
                        selected,
                        id,
                        target,
                        Step::Split {
                            source: id,
                            new_stack,
                        },
                        actual,
                    );
                    return;
                }
                assert!(
                    !probe.integrated,
                    "combined route needs a confirmed bandage stack with quantity >= 2"
                );
                info!("GRID PROBE SKIP split/merge: no confirmed bandage stack with quantity >= 2");
                info!("GRID PROBE PASS move/rotate only");
            }
            Step::Split { source, new_stack } => {
                let (&new_id, item) = actual
                    .iter()
                    .find(|(id, _)| !check.expected.contains_key(id))
                    .expect("split must publish a distinct instance");
                assert_eq!(item, &new_stack);
                actual.remove(&new_id);
                assert_eq!(actual, check.expected);
                let original = actual.get_mut(&source).unwrap();
                original.quantity += 1;
                let target = original.placement;
                probe.route(grid, selected, new_id, target, Step::Merge, actual);
                return;
            }
            Step::Merge => {
                assert_eq!(actual, check.expected);
                info!("GRID PROBE PASS move/rotate and split/merge");
            }
        }
        if probe.integrated {
            keyboard.write(emit(KeyCode::Tab, ButtonState::Pressed));
            probe.release.push(KeyCode::Tab);
            probe.closing = true;
        } else {
            probe.finished = true;
            exit.write(AppExit::Success);
        }
        return;
    }
    let Some((id, target)) = grid.items.iter().find_map(|(id, item)| {
        if probe.integrated && item.definition != "food.water500" {
            return None;
        }
        let footprint = planned_items().get(item.definition.as_str())?.footprint;
        if footprint.dimensions() != [1, 2] {
            return None;
        }
        let turned = match item.placement.rotation {
            Rotation::Upright => Rotation::QuarterTurn,
            Rotation::QuarterTurn => Rotation::Upright,
        };
        destination(grid, *id, footprint, turned, false).map(|p| (*id, p))
    }) else {
        return;
    };
    probe.run = Some((key, time.elapsed_secs_f64(), sequences));
    actual.get_mut(&id).unwrap().placement = target;
    info!(
        "GRID PROBE moving {id:?} to {target:?} from revision {}",
        grid.version.revision
    );
    probe.route(grid, selected, id, target, Step::Move, actual);
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::catalog::{
        CompartmentId,
        item_inventory::{Condition, ItemContents, VersionedCompartment},
    };

    #[test]
    fn inventory_grid_probe_handoff_waits_until_tab_has_closed_the_panel() {
        let mut app = App::new();
        app.add_message::<AppExit>()
            .add_message::<KeyboardInput>()
            .init_resource::<State>()
            .init_resource::<Panel>()
            .init_resource::<LocalInput>()
            .init_resource::<Time<Real>>()
            .insert_resource(PlayerId(7))
            .insert_resource(ClientSession {
                raid: common::raid::RaidState {
                    phase: common::raid::RaidPhase::Active,
                    ..default()
                },
                players: vec![common::ClientData {
                    id: 7,
                    participant: common::raid::Participant {
                        status: common::raid::RaidStatus::Active,
                        ..default()
                    },
                    ..default()
                }],
                ..default()
            })
            .add_plugins(Plugin { integrated: true });
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        let emitted = |app: &mut App| {
            app.world_mut()
                .resource_mut::<Messages<KeyboardInput>>()
                .drain()
                .map(|key| (key.key_code, key.state))
                .collect::<Vec<_>>()
        };
        app.update();
        assert!(
            emitted(&mut app).is_empty(),
            "loot must request the grid first"
        );
        assert!(!app.world_mut().resource_mut::<Probe>().ready_to_resume());
        app.update();
        assert_eq!(emitted(&mut app), [(KeyCode::Tab, ButtonState::Pressed)]);
        {
            // Isolate the handoff from transactions (already covered by
            // inventory_grid tests): model the frame after Merge confirmation.
            let mut probe = app.world_mut().resource_mut::<Probe>();
            probe.closing = true;
        }
        app.update();
        assert_eq!(emitted(&mut app), [(KeyCode::Tab, ButtonState::Released)]);
        assert!(!app.world_mut().resource_mut::<Probe>().ready_to_resume());
        app.update();
        assert!(app.world_mut().resource_mut::<Probe>().ready_to_resume());
        assert!(app.world().resource::<Messages<AppExit>>().is_empty());
    }

    #[test]
    fn inventory_grid_probe_plans_rotated_move_around_occupied_cells() {
        let water = ItemInstanceId(1);
        let bandage = ItemInstanceId(2);
        let item = |definition: &str, x, y| ProjectedItem {
            definition: definition.into(),
            condition: Condition::Intact,
            quantity: 1,
            placement: Placement {
                x,
                y,
                rotation: Rotation::Upright,
            },
            contents: ItemContents::None,
        };
        let grid = InventorySnapshot {
            version: VersionedCompartment {
                id: CompartmentId(7),
                revision: 12,
            },
            size: [3, 2],
            items: vec![
                (water, item("food.water500", 0, 0)),
                (bandage, item("med.bandage", 1, 0)),
            ],
        };
        let target = destination(
            &grid,
            water,
            Footprint::new(1, 2).unwrap(),
            Rotation::QuarterTurn,
            false,
        )
        .unwrap();
        assert_eq!(
            target,
            Placement {
                x: 0,
                y: 1,
                rotation: Rotation::QuarterTurn
            }
        );
        let mut probe = Probe::default();
        probe.route(&grid, bandage, water, target, Step::Move, Items::new());
        assert_eq!(probe.check.unwrap().revision, 13);
        assert_eq!(
            probe.keys,
            [
                vec![KeyCode::ArrowDown],
                vec![KeyCode::Enter],
                vec![KeyCode::KeyR],
                vec![KeyCode::ArrowDown],
                vec![KeyCode::Enter],
            ]
        );
    }
}
