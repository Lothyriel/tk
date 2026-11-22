//! Native client probe, driven by synthetic keyboard events. Never writes an
//! inventory intent or replica. The operator collects loot and opens the grid.
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

pub(crate) struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Probe>()
            .add_systems(Startup, || {
                info!("GRID PROBE: collect a 1x2 item, then open the backpack grid with Tab");
            })
            .add_systems(Update, drive.after(ClientSystems::Present));
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
struct Probe {
    run: Option<(SessionKey, f64, [u32; 2])>,
    keys: VecDeque<Vec<KeyCode>>,
    release: Vec<KeyCode>,
    check: Option<Check>,
    finished: bool,
}

impl Probe {
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

fn drive(
    mut probe: ResMut<Probe>,
    (session, player, inventory, panel, input, time): Live,
    window: Single<(Entity, &Window), With<PrimaryWindow>>,
    mut keyboard: MessageWriter<KeyboardInput>,
    mut exit: MessageWriter<AppExit>,
) {
    if probe.finished {
        return;
    }
    let (window_id, window) = *window;
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
    let (Some(grid), Some(key), Some(selected)) = (
        inventory.owned_grid(session.raid.raid_id, player.0),
        inventory.grid_session(),
        panel.selected,
    ) else {
        return;
    };
    let emit = |key_code, state| KeyboardInput {
        key_code,
        window: window_id,
        state,
        repeat: false,
        text: None,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
    };
    if !probe.release.is_empty() {
        for key in probe.release.drain(..) {
            keyboard.write(emit(key, ButtonState::Released));
        }
        return;
    }
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
            || panel.grid_feedback() != Some(check.step.feedback())
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
        probe.finished = true;
        exit.write(AppExit::Success);
        return;
    }
    let Some((id, target)) = grid.items.iter().find_map(|(id, item)| {
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
