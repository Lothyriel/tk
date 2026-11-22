use super::Panel;
use crate::sync::inventory::{Operate, State};
use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    ui::{FocusPolicy, RelativeCursorPosition},
};
use bevy_renet2::prelude::RenetClient;
use common::catalog::{
    ItemInstanceId,
    item_grid::{Footprint, Placement, PlacementError, Rotation},
    item_inventory::{
        GridLocation, InventoryAction, InventorySnapshot, OperationResult, ProjectedItem,
        SessionKey, VersionedCompartment,
    },
    item_manifest::planned_items,
};

#[derive(Clone, Copy)]
pub(super) struct Drag {
    session: SessionKey,
    version: VersionedCompartment,
    item: ItemInstanceId,
    target: Option<Placement>,
    rotation: Rotation,
    mouse: bool,
    grab: [u8; 2],
    split: Option<Split>,
    hover: Option<[u8; 2]>,
}

#[derive(Clone, Copy)]
struct Split {
    quantity: u16,
    maximum: u16,
    choosing: bool,
}

#[derive(Clone, Copy)]
enum Expected {
    Move,
    Split {
        remainder: u16,
        quantity: u16,
    },
    Merge {
        target: ItemInstanceId,
        remainder: u16,
        total: u16,
    },
}

#[derive(Clone, Copy)]
pub(super) struct Pending {
    session: SessionKey,
    version: VersionedCompartment,
    operation: u64,
    item: ItemInstanceId,
    target: Placement,
    expected: Expected,
}

#[derive(SystemParam)]
pub(in crate::input) struct PointerInput<'w, 's> {
    buttons: Option<Res<'w, ButtonInput<MouseButton>>>,
    cursor: Query<'w, 's, &'static RelativeCursorPosition, With<Board>>,
    quantity: Query<'w, 's, (&'static QuantityButton, &'static RelativeCursorPosition)>,
}

#[derive(Component, Clone, Copy)]
enum QuantityButton {
    Less,
    More,
    Confirm,
    Cancel,
}

#[derive(Component)]
pub(in crate::input) struct QuantityControls;

#[derive(Component)]
pub(in crate::input) struct Board;

#[derive(Component)]
pub(in crate::input) struct GridRoot;

#[derive(Component)]
pub(in crate::input) struct Feedback;

#[derive(Component)]
pub(in crate::input) struct Tile;

fn footprint(item: &ProjectedItem) -> Footprint {
    planned_items()[item.definition.as_str()].footprint
}

fn known_grid(grid: &&InventorySnapshot) -> bool {
    grid.size.into_iter().all(|size| (1..=16).contains(&size))
        && grid
            .items
            .iter()
            .all(|(_, item)| planned_items().contains_key(item.definition.as_str()))
}

fn dimensions(item: &ProjectedItem, rotation: Rotation) -> [u8; 2] {
    let [w, h] = footprint(item).dimensions();
    match rotation {
        Rotation::Upright => [w, h],
        Rotation::QuarterTurn => [h, w],
    }
}

fn cell_at(cursor: &RelativeCursorPosition, size: [u8; 2]) -> Option<[u8; 2]> {
    let point = cursor.normalized? + Vec2::splat(0.5);
    (cursor.cursor_over && point.min_element() >= 0.0 && point.max_element() < 1.0).then(|| {
        [
            (point.x * f32::from(size[0])) as u8,
            (point.y * f32::from(size[1])) as u8,
        ]
    })
}

#[cfg(test)]
fn check(
    grid: &InventorySnapshot,
    item: ItemInstanceId,
    target: Placement,
) -> Option<Result<(), PlacementError>> {
    check_placement(grid, item, target, false)
}

fn check_placement(
    grid: &InventorySnapshot,
    item: ItemInstanceId,
    target: Placement,
    split: bool,
) -> Option<Result<(), PlacementError>> {
    let (_, source) = grid.items.iter().find(|(id, _)| *id == item)?;
    Some(
        target.area(footprint(source)).validate(
            Footprint::new(grid.size[0], grid.size[1])?,
            grid.items
                .iter()
                .filter(|(id, _)| split || *id != item)
                .map(|(_, other)| other.placement.area(footprint(other))),
        ),
    )
}

fn item_at(grid: &InventorySnapshot, [x, y]: [u8; 2]) -> Option<&(ItemInstanceId, ProjectedItem)> {
    grid.items.iter().find(|(_, item)| {
        let [w, h] = dimensions(item, item.placement.rotation);
        x >= item.placement.x
            && x - item.placement.x < w
            && y >= item.placement.y
            && y - item.placement.y < h
    })
}

fn merge_plan(source: &ProjectedItem, target: &ProjectedItem) -> Result<(u16, u16), &'static str> {
    if source.definition != target.definition
        || source.condition != target.condition
        || source.contents != target.contents
    {
        return Err("Stacks are incompatible");
    }
    let capacity = planned_items()[target.definition.as_str()].stack_max;
    let moved = source
        .quantity
        .min(capacity.saturating_sub(target.quantity));
    if moved == 0 {
        return Err("Target stack is full");
    }
    Ok((source.quantity - moved, target.quantity + moved))
}

pub(super) fn reconcile(panel: &mut Panel, inventory: &State, raid: u32, client: u64) {
    let grid = inventory.owned_grid(raid, client);
    let session = inventory.grid_session();
    if let Some(drag) = panel.drag
        && (session != Some(drag.session) || grid.is_none_or(|grid| grid.version != drag.version))
    {
        panel.drag = None;
        panel.feedback = Some("Move cancelled: inventory changed");
    }
    let Some(pending) = panel.pending else { return };
    if session != Some(pending.session) {
        panel.pending = None;
        panel.feedback = None;
        return;
    }
    match inventory.outcome(pending.operation) {
        None => (),
        Some(OperationResult::Committed) => {
            // The ack alone cannot move the visible tile.
            if let Some(grid) = grid.filter(|grid| grid.version.revision > pending.version.revision)
            {
                let item = |id| {
                    grid.items
                        .iter()
                        .find(|(other, _)| *other == id)
                        .map(|(_, item)| item)
                };
                let confirmed = match pending.expected {
                    Expected::Move => item(pending.item)
                        .filter(|i| i.placement == pending.target)
                        .map(|_| "Move confirmed"),
                    Expected::Split {
                        remainder,
                        quantity,
                    } => item(pending.item)
                        .filter(|i| {
                            i.quantity == remainder
                                && grid.items.iter().any(|(id, split)| {
                                    *id != pending.item
                                        && split.placement == pending.target
                                        && split.quantity == quantity
                                        && split.definition == i.definition
                                })
                        })
                        .map(|_| "Split confirmed"),
                    Expected::Merge {
                        target,
                        remainder,
                        total,
                    } => item(target)
                        .filter(|i| {
                            i.quantity == total
                                && item(pending.item).map_or(0, |i| i.quantity) == remainder
                        })
                        .map(|_| {
                            if remainder == 0 {
                                "Merge confirmed"
                            } else {
                                "Merge confirmed: remainder in source stack"
                            }
                        }),
                };
                panel.pending = None;
                panel.feedback = Some(confirmed.unwrap_or("Inventory changed after operation"));
            }
        }
        Some(OperationResult::Conflict | OperationResult::Expired) => {
            panel.pending = None;
            panel.feedback = Some(if matches!(pending.expected, Expected::Move) {
                "Move rejected: inventory changed"
            } else {
                "Stack operation rejected: inventory changed"
            });
        }
        Some(_) => {
            panel.pending = None;
            panel.feedback = Some(if matches!(pending.expected, Expected::Move) {
                "Move rejected by server"
            } else {
                "Stack operation rejected by server"
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn interact(
    panel: &mut Panel,
    inventory: &mut State,
    client: &mut RenetClient,
    raid: u32,
    owner: u64,
    keys: &ButtonInput<KeyCode>,
    pointer: &PointerInput,
) -> bool {
    if panel.pending.is_some() {
        return true;
    }
    let (Some(grid), Some(session)) = (
        inventory.owned_grid(raid, owner).filter(known_grid),
        inventory.grid_session(),
    ) else {
        return false;
    };
    let cell = pointer
        .cursor
        .single()
        .ok()
        .and_then(|cursor| cell_at(cursor, grid.size));
    let mouse = pointer.buttons.as_deref();
    if panel.drag.is_none() {
        let pressed = mouse.is_some_and(|buttons| buttons.just_pressed(MouseButton::Left));
        let keyboard = keys.just_pressed(KeyCode::Enter);
        let source = if pressed {
            cell.and_then(|cell| item_at(grid, cell))
        } else if keyboard {
            grid.items
                .iter()
                .find(|(id, _)| Some(*id) == panel.selected)
        } else {
            None
        };
        if let Some((item, source)) = source {
            panel.selected = Some(*item);
            panel.feedback = None;
            let split = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
            if split && source.quantity <= 1 {
                panel.feedback = Some("Need at least two items to split");
                return true;
            }
            panel.drag = Some(Drag {
                session,
                version: grid.version,
                item: *item,
                target: Some(source.placement),
                rotation: source.placement.rotation,
                mouse: pressed,
                grab: cell.filter(|_| pressed).map_or([0, 0], |[x, y]| {
                    [x - source.placement.x, y - source.placement.y]
                }),
                split: split.then(|| Split {
                    quantity: 1,
                    maximum: source.quantity - 1,
                    choosing: true,
                }),
                hover: None,
            });
            return true;
        }
        return false;
    }
    let drag = panel.drag.as_mut().unwrap();
    if let Some(split) = drag.split.as_mut().filter(|s| s.choosing) {
        let button = mouse
            .is_some_and(|b| b.just_pressed(MouseButton::Left))
            .then(|| {
                pointer
                    .quantity
                    .iter()
                    .find_map(|(button, cursor)| cursor.cursor_over.then_some(*button))
            })
            .flatten();
        if matches!(button, Some(QuantityButton::Cancel)) {
            panel.drag = None;
            panel.feedback = Some("Split cancelled");
        } else if keys.just_pressed(KeyCode::Enter)
            || matches!(button, Some(QuantityButton::Confirm))
        {
            split.choosing = false;
            // The quantity confirmation is not a destination click.
            drag.mouse = false;
            drag.grab = [0, 0];
        } else if keys.just_pressed(KeyCode::ArrowRight)
            || matches!(button, Some(QuantityButton::More))
        {
            split.quantity = split.quantity.saturating_add(1).min(split.maximum);
        } else if keys.just_pressed(KeyCode::ArrowLeft)
            || matches!(button, Some(QuantityButton::Less))
        {
            split.quantity = split.quantity.saturating_sub(1).max(1);
        }
        return true;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        let (_, item) = grid.items.iter().find(|(id, _)| *id == drag.item).unwrap();
        let [w, h] = dimensions(item, drag.rotation);
        let [x, y] = drag.grab;
        drag.grab = if drag.rotation == Rotation::Upright {
            [h - 1 - y, x]
        } else {
            [y, w - 1 - x]
        };
        drag.rotation = match drag.rotation {
            Rotation::Upright => Rotation::QuarterTurn,
            Rotation::QuarterTurn => Rotation::Upright,
        };
    }
    if drag.mouse {
        drag.target = cell.and_then(|[x, y]| {
            Some(Placement {
                x: x.checked_sub(drag.grab[0])?,
                y: y.checked_sub(drag.grab[1])?,
                rotation: drag.rotation,
            })
        });
    } else if let Some(target) = &mut drag.target {
        let step = |a, b| i16::from(keys.just_pressed(a)) - i16::from(keys.just_pressed(b));
        target.x = (i16::from(target.x) + step(KeyCode::ArrowRight, KeyCode::ArrowLeft))
            .clamp(0, i16::from(grid.size[0]) - 1) as u8;
        target.y = (i16::from(target.y) + step(KeyCode::ArrowDown, KeyCode::ArrowUp))
            .clamp(0, i16::from(grid.size[1]) - 1) as u8;
        target.rotation = drag.rotation;
    }
    let split_click =
        drag.split.is_some() && mouse.is_some_and(|b| b.just_pressed(MouseButton::Left));
    if split_click {
        drag.target = cell.map(|[x, y]| Placement {
            x,
            y,
            rotation: drag.rotation,
        });
    }
    drag.hover = if drag.mouse {
        cell
    } else {
        drag.target.map(|p| [p.x, p.y])
    };
    let submit = if drag.mouse {
        mouse.is_some_and(|buttons| buttons.just_released(MouseButton::Left))
    } else {
        keys.just_pressed(KeyCode::Enter) || split_click
    };
    if !submit {
        return true;
    }
    let drag = panel.drag.take().unwrap();
    let Some(target) = drag.target else {
        panel.feedback = Some("Move cancelled outside grid");
        return true;
    };
    let id = grid.version.id;
    let destination = GridLocation {
        compartment: id,
        placement: target,
    };
    let (_, source) = grid.items.iter().find(|(id, _)| *id == drag.item).unwrap();
    let anchor = drag.hover.unwrap_or([target.x, target.y]);
    let (action, expected) = if drag.split.is_none()
        && let Some((other, stack)) = item_at(grid, anchor).filter(|(other, _)| *other != drag.item)
    {
        let (remainder, total) = match merge_plan(source, stack) {
            Ok(plan) => plan,
            Err(message) => {
                panel.feedback = Some(message);
                return true;
            }
        };
        (
            InventoryAction::Merge {
                source: drag.item,
                target: *other,
            },
            Expected::Merge {
                target: *other,
                remainder,
                total,
            },
        )
    } else {
        let verdict = check_placement(grid, drag.item, target, drag.split.is_some());
        if verdict != Some(Ok(())) {
            panel.feedback = Some(reason(verdict));
            return true;
        }
        if let Some(split) = drag.split {
            (
                InventoryAction::Split {
                    item: drag.item,
                    quantity: split.quantity,
                    destination,
                },
                Expected::Split {
                    remainder: source.quantity - split.quantity,
                    quantity: split.quantity,
                },
            )
        } else {
            if source.placement == target {
                return true;
            }
            (
                InventoryAction::Move {
                    item: drag.item,
                    destination,
                },
                Expected::Move,
            )
        }
    };
    let intent = Operate {
        action,
        scope: [id; 2],
    };
    if let Some(operation) = inventory.send_operation(client, &intent) {
        panel.pending = Some(Pending {
            session,
            version: drag.version,
            operation,
            item: drag.item,
            target,
            expected,
        });
    } else {
        panel.feedback = Some("Move not sent");
    }
    true
}

fn reason(verdict: Option<Result<(), PlacementError>>) -> &'static str {
    match verdict {
        Some(Ok(())) => "Valid placement",
        Some(Err(PlacementError::Occupied)) => "Cell occupied",
        Some(Err(PlacementError::OutsideGrid)) => "Item exceeds grid edge",
        None => "Outside grid",
    }
}

pub(in crate::input) fn setup(mut commands: Commands) {
    commands
        .spawn((
            GridRoot,
            GlobalZIndex(220),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                right: Val::Px(20.0),
                top: Val::Px(68.0),
                padding: UiRect::all(Val::Px(12.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                width: Val::Px(300.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.02, 0.04, 0.06)),
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("BACKPACK GRID\nDrag or Enter to move · R rotate\nShift+drag/Enter split · drop on stack to merge"),
                TextFont::from_font_size(13.0),
            ));
            parent.spawn((
                Board,
                RelativeCursorPosition::default(),
                Node {
                    width: Val::Percent(100.0),
                    aspect_ratio: Some(1.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.10, 0.13, 0.16)),
            ));
            parent.spawn((Feedback, Text::default(), TextFont::from_font_size(14.0)));
            parent.spawn((QuantityControls, Node { display: Display::None, column_gap: Val::Px(5.0), ..default() }))
                .with_children(|buttons| {
                    for (action, label) in [
                        (QuantityButton::Less, "−"),
                        (QuantityButton::More, "+"),
                        (QuantityButton::Confirm, "Choose"),
                        (QuantityButton::Cancel, "Cancel"),
                    ] {
                        buttons.spawn((
                            action, RelativeCursorPosition::default(),
                            Node { padding: UiRect::all(Val::Px(7.0)), ..default() },
                            BackgroundColor(Color::srgb(0.17, 0.29, 0.40)),
                        )).with_child((FocusPolicy::Pass, Text::new(label), TextFont::from_font_size(14.0)));
                    }
                });
        });
}

fn rectangle(cell: Placement, size: [u8; 2], grid: [u8; 2]) -> Node {
    let [x, y] = grid.map(|n| 100.0 / f32::from(n));
    Node {
        position_type: PositionType::Absolute,
        left: Val::Percent(f32::from(cell.x) * x),
        top: Val::Percent(f32::from(cell.y) * y),
        width: Val::Percent(f32::from(size[0]) * x),
        height: Val::Percent(f32::from(size[1]) * y),
        border: UiRect::all(Val::Px(1.0)),
        overflow: Overflow::clip(),
        ..default()
    }
}

type DrawKey = (
    SessionKey,
    VersionedCompartment,
    Option<ItemInstanceId>,
    Option<(ItemInstanceId, Placement, bool)>,
    bool,
);

type BoardNode<'w, 's> =
    Single<'w, 's, (Entity, &'static mut Node), (With<Board>, Without<GridRoot>)>;
type QuantityNode<'w, 's> =
    Single<'w, 's, &'static mut Node, (With<QuantityControls>, Without<GridRoot>, Without<Board>)>;

#[allow(clippy::too_many_arguments)]
pub(in crate::input) fn present(
    mut commands: Commands,
    (panel, inventory): (Res<Panel>, Res<State>),
    (session, player): (Res<crate::sync::ClientSession>, Res<common::PlayerId>),
    mut root: Single<&mut Node, With<GridRoot>>,
    mut board: BoardNode,
    mut quantity: QuantityNode,
    mut message: Single<&mut Text, With<Feedback>>,
    tiles: Query<Entity, With<Tile>>,
    mut last: Local<Option<DrawKey>>,
) {
    let grid = inventory
        .owned_grid(session.raid.raid_id, player.0)
        .filter(known_grid);
    let visible = panel.open && panel.inspection.is_none();
    let choosing = panel.drag.and_then(|d| d.split).filter(|s| s.choosing);
    quantity.display = if visible && choosing.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    root.display = if visible {
        Display::Flex
    } else {
        Display::None
    };
    if !visible {
        return;
    }
    let (Some(grid), Some(key)) = (grid, inventory.grid_session()) else {
        board.1.display = Display::None;
        let label = panel.feedback.unwrap_or("Waiting for confirmed backpack");
        if message.as_str() != label {
            ***message = label.into();
        }
        return;
    };
    board.1.display = Display::Flex;
    let merging = panel.drag.filter(|d| d.split.is_none()).and_then(|d| {
        let (id, item) = item_at(grid, d.hover?)?;
        if *id == d.item {
            return None;
        }
        let (_, source) = grid.items.iter().find(|(id, _)| *id == d.item)?;
        Some((item.placement, merge_plan(source, item)))
    });
    let ghost = panel
        .drag
        .filter(|_| choosing.is_none())
        .and_then(|drag| {
            drag.target
                .map(|target| (drag.item, merging.as_ref().map_or(target, |m| m.0), false))
        })
        .or_else(|| panel.pending.map(|p| (p.item, p.target, true)));
    let split = panel.drag.and_then(|d| d.split);
    let verdict =
        ghost.and_then(|(id, target, _)| check_placement(grid, id, target, split.is_some()));
    let valid = merging
        .as_ref()
        .map_or(verdict == Some(Ok(())), |(_, plan)| plan.is_ok());
    let label = if let Some(split) = choosing {
        format!(
            "Split {} / {} · arrows or −/+ · Enter to choose",
            split.quantity, split.maximum
        )
    } else if panel.pending.is_some() {
        "Waiting for server confirmation".to_owned()
    } else if let Some((_, plan)) = merging {
        match plan {
            Ok((remainder, total)) => {
                format!("Merge: target ×{total} · source remainder ×{remainder}")
            }
            Err(reason) => reason.to_owned(),
        }
    } else if let Some(split) = split {
        format!("Place split ×{} · {}", split.quantity, reason(verdict))
    } else if panel.drag.is_some() {
        reason(verdict).to_owned()
    } else {
        panel
            .feedback
            .unwrap_or("Select an item to move")
            .to_owned()
    };
    if message.as_str() != label {
        ***message = label;
    }
    let draw = (key, grid.version, panel.selected, ghost, valid);
    if *last == Some(draw) {
        return;
    }
    *last = Some(draw);
    board.1.aspect_ratio = Some(f32::from(grid.size[0]) / f32::from(grid.size[1]));
    for entity in &tiles {
        commands.entity(entity).despawn();
    }
    let mut parent = commands.entity(board.0);
    parent.with_children(|board| {
        for y in 0..grid.size[1] {
            for x in 0..grid.size[0] {
                board.spawn((
                    Tile,
                    Pickable::IGNORE,
                    FocusPolicy::Pass,
                    rectangle(
                        Placement {
                            x,
                            y,
                            rotation: Rotation::Upright,
                        },
                        [1, 1],
                        grid.size,
                    ),
                    BorderColor::all(Color::srgb(0.19, 0.23, 0.30)),
                ));
            }
        }
        for (id, item) in &grid.items {
            let entry = &planned_items()[item.definition.as_str()];
            board
                .spawn((
                    Tile,
                    Pickable::IGNORE,
                    FocusPolicy::Pass,
                    rectangle(
                        item.placement,
                        dimensions(item, item.placement.rotation),
                        grid.size,
                    ),
                    BackgroundColor(if panel.selected == Some(*id) {
                        Color::srgb(0.23, 0.38, 0.48)
                    } else {
                        Color::srgb(0.13, 0.22, 0.30)
                    }),
                    BorderColor::all(Color::srgb(0.48, 0.60, 0.70)),
                ))
                .with_child((
                    Pickable::IGNORE,
                    FocusPolicy::Pass,
                    Text::new(format!("{} ×{}", entry.name, item.quantity)),
                    TextFont::from_font_size(12.0),
                ));
        }
        if let Some((id, target, pending)) = ghost
            && let Some((_, item)) = grid.items.iter().find(|(other, _)| *other == id)
        {
            board.spawn((
                Tile,
                Pickable::IGNORE,
                FocusPolicy::Pass,
                rectangle(target, dimensions(item, target.rotation), grid.size),
                BorderColor::all(Color::WHITE),
                BackgroundColor(if pending {
                    Color::srgba(0.72, 0.58, 0.12, 0.5)
                } else if valid {
                    Color::srgba(0.06, 0.65, 0.38, 0.5)
                } else {
                    Color::srgba(0.9, 0.2, 0.20, 0.5)
                }),
            ));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::window::PrimaryWindow;
    use bevy_renet2::prelude::RenetServer;
    use common::{
        ClientData, PlayerId,
        catalog::{
            CompartmentId,
            item_inventory::{Condition, Inventory, InventoryAuthority, OperationSession},
        },
        data,
        raid::RaidPhase,
        replication::inventory::{CHANNEL, InventoryRequest, InventoryUpdate, UpdateBody},
    };

    fn placement(x: u8, y: u8) -> Placement {
        Placement {
            x,
            y,
            rotation: Rotation::Upright,
        }
    }

    const KEY: SessionKey = SessionKey {
        raid_epoch: 1,
        session_epoch: 1,
    };
    const OWN: CompartmentId = CompartmentId(7);

    fn fixture() -> (App, InventoryAuthority, ItemInstanceId) {
        fixture_with_stacks(false)
    }

    fn fixture_with_stacks(stacks: bool) -> (App, InventoryAuthority, ItemInstanceId) {
        let mut source = Inventory::default();
        source
            .register_compartment(OWN, Footprint::new(4, 4).unwrap(), None)
            .unwrap();
        let mut seed = |definition, placement, quantity| {
            source
                .insert_seed(
                    definition,
                    Condition::Intact,
                    quantity,
                    GridLocation {
                        compartment: OWN,
                        placement,
                    },
                )
                .unwrap()
        };
        let movable = seed(
            if stacks {
                "med.bandage"
            } else {
                "food.water500"
            },
            placement(0, 0),
            if stacks { 3 } else { 1 },
        );
        seed("med.bandage", placement(2, 0), if stacks { 2 } else { 1 });
        let authority = InventoryAuthority::new(1, source);
        let mut state = State::default();
        for body in [
            UpdateBody::Begin,
            UpdateBody::Snapshot(authority.snapshot(OWN, |id| id == OWN).unwrap()),
        ] {
            state.apply(InventoryUpdate { session: KEY, body });
        }
        let mut session = crate::sync::ClientSession::default();
        session.raid.phase = RaidPhase::Active;
        session.raid.raid_id = 1;
        let mut actor = ClientData { id: 7, ..default() };
        actor.participant.begin();
        session.players.push(actor);
        let mut client = RenetClient::new(data::renet_config(), false);
        client.set_connected();
        let mut app = App::new();
        app.insert_resource(client)
            .insert_resource(session)
            .insert_resource(state)
            .insert_resource(PlayerId(7))
            .init_resource::<Time<Real>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Panel>()
            .add_systems(Startup, setup)
            .add_systems(Update, (super::super::update, present).chain());
        app.world_mut().spawn((
            PrimaryWindow,
            Window {
                focused: true,
                ..default()
            },
        ));
        key(&mut app, KeyCode::Tab);
        (app, authority, movable)
    }

    fn key(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(key);
        app.update();
    }

    fn mouse(app: &mut App, point: Vec2, pressed: bool) {
        let mut cursor = app
            .world_mut()
            .query_filtered::<&mut RelativeCursorPosition, With<Board>>();
        let mut cursor = cursor.single_mut(app.world_mut()).unwrap();
        cursor.normalized = Some(point);
        cursor.cursor_over = point.abs().max_element() < 0.5;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        buttons.clear();
        if pressed {
            buttons.press(MouseButton::Left);
        } else {
            buttons.release(MouseButton::Left);
        }
        app.update();
    }

    fn transfer(app: &mut App, server: &mut RenetServer) -> Option<InventoryRequest> {
        for packet in app
            .world_mut()
            .resource_mut::<RenetClient>()
            .get_packets_to_send()
        {
            server.process_packet_from(packet, 7).unwrap();
        }
        server
            .receive_message(7, CHANNEL)
            .map(|bytes| InventoryRequest::decode(&bytes).unwrap())
    }

    #[test]
    fn inventory_grid_keyboard_move_waits_for_authoritative_placement() {
        let (mut app, mut authority, item) = fixture();
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::KeyR);
        key(&mut app, KeyCode::ArrowDown);
        key(&mut app, KeyCode::Enter);
        let mut server = RenetServer::new(data::renet_config());
        server.add_connection(7, false);
        let Some(InventoryRequest::Operate(op)) = transfer(&mut app, &mut server) else {
            panic!("expected move");
        };
        assert_eq!(
            op.action,
            InventoryAction::Move {
                item,
                destination: GridLocation {
                    compartment: OWN,
                    placement: Placement {
                        x: 0,
                        y: 1,
                        rotation: Rotation::QuarterTurn
                    }
                },
            }
        );
        assert!(transfer(&mut app, &mut server).is_none());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        let reply = authority.apply(&mut OperationSession::new(KEY), op, |id| id == OWN);
        assert_eq!(reply.result, OperationResult::Committed);
        let result = InventoryUpdate {
            session: KEY,
            body: UpdateBody::Result {
                id: op.id,
                result: reply.result,
            },
        };
        app.world_mut().resource_mut::<State>().apply(result);
        app.update();
        assert!(app.world().resource::<Panel>().pending.is_some());
        assert_eq!(
            app.world()
                .resource::<State>()
                .owned_grid(1, 7)
                .unwrap()
                .items[0]
                .1
                .placement,
            placement(0, 0)
        );
        app.world_mut()
            .resource_mut::<State>()
            .apply(InventoryUpdate {
                session: KEY,
                body: UpdateBody::Snapshot(authority.snapshot(OWN, |id| id == OWN).unwrap()),
            });
        app.update();
        assert!(app.world().resource::<Panel>().pending.is_none());
        assert_eq!(
            app.world().resource::<Panel>().feedback,
            Some("Move confirmed")
        );
    }

    #[test]
    fn inventory_grid_mouse_outside_and_escape_never_submit_drop() {
        let (mut app, authority, item) = fixture();
        let snapshot = authority.snapshot(OWN, |id| id == OWN).unwrap();
        assert_eq!(
            check(
                &snapshot,
                item,
                Placement {
                    x: 1,
                    y: 0,
                    rotation: Rotation::QuarterTurn
                }
            ),
            Some(Err(PlacementError::Occupied))
        );
        assert_eq!(
            check(&snapshot, item, placement(0, 3)),
            Some(Err(PlacementError::OutsideGrid))
        );
        mouse(&mut app, Vec2::splat(-0.375), true);
        assert!(app.world().resource::<Panel>().drag.is_some());
        mouse(&mut app, Vec2::splat(0.7), false);
        assert_eq!(
            app.world().resource::<Panel>().feedback,
            Some("Move cancelled outside grid")
        );
        mouse(&mut app, Vec2::splat(-0.375), true);
        key(&mut app, KeyCode::Escape);
        let panel = app.world().resource::<Panel>();
        assert!(panel.open && panel.consumed_escape && panel.drag.is_none());
        let mut server = RenetServer::new(data::renet_config());
        server.add_connection(7, false);
        assert!(transfer(&mut app, &mut server).is_none());
    }

    #[test]
    fn inventory_grid_conflict_clears_preview_without_replaying_move() {
        let (mut app, _, _) = fixture();
        mouse(&mut app, Vec2::splat(-0.375), true);
        mouse(&mut app, Vec2::new(-0.125, 0.125), false);
        let mut server = RenetServer::new(data::renet_config());
        server.add_connection(7, false);
        let Some(InventoryRequest::Operate(op)) = transfer(&mut app, &mut server) else {
            panic!("expected move");
        };
        app.world_mut()
            .resource_mut::<State>()
            .apply(InventoryUpdate {
                session: KEY,
                body: UpdateBody::Result {
                    id: op.id,
                    result: OperationResult::Conflict,
                },
            });
        app.update();
        let panel = app.world().resource::<Panel>();
        assert!(panel.pending.is_none());
        assert_eq!(panel.feedback, Some("Move rejected: inventory changed"));
        assert!(app.world().resource::<State>().owned_grid(1, 7).is_none());
        assert!(transfer(&mut app, &mut server).is_none());
    }

    fn apply_stack_operation(
        app: &mut App,
        authority: &mut InventoryAuthority,
        op: common::catalog::item_inventory::InventoryOperation,
    ) {
        let reply = authority.apply(&mut OperationSession::new(KEY), op, |id| id == OWN);
        assert_eq!(reply.result, OperationResult::Committed);
        for body in [
            UpdateBody::Snapshot(authority.snapshot(OWN, |id| id == OWN).unwrap()),
            UpdateBody::Result {
                id: op.id,
                result: reply.result,
            },
        ] {
            app.world_mut()
                .resource_mut::<State>()
                .apply(InventoryUpdate { session: KEY, body });
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
    }

    #[test]
    fn inventory_grid_split_clamps_quantity_and_waits_for_confirmed_stack() {
        let (mut app, mut authority, item) = fixture_with_stacks(true);
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::ShiftLeft);
        keys.press(KeyCode::Enter);
        app.update();
        key(&mut app, KeyCode::ArrowLeft);
        assert_eq!(
            app.world()
                .resource::<Panel>()
                .drag
                .unwrap()
                .split
                .unwrap()
                .quantity,
            1
        );
        key(&mut app, KeyCode::ArrowRight);
        key(&mut app, KeyCode::ArrowRight);
        assert_eq!(
            app.world()
                .resource::<Panel>()
                .drag
                .unwrap()
                .split
                .unwrap()
                .quantity,
            2
        );
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::ArrowDown);
        key(&mut app, KeyCode::Enter);
        let mut server = RenetServer::new(data::renet_config());
        server.add_connection(7, false);
        let Some(InventoryRequest::Operate(op)) = transfer(&mut app, &mut server) else {
            panic!("expected Split");
        };
        assert_eq!(
            op.action,
            InventoryAction::Split {
                item,
                quantity: 2,
                destination: GridLocation {
                    compartment: OWN,
                    placement: placement(0, 1)
                }
            }
        );
        assert_eq!(
            app.world()
                .resource::<State>()
                .owned_grid(1, 7)
                .unwrap()
                .items[0]
                .1
                .quantity,
            3
        );
        apply_stack_operation(&mut app, &mut authority, op);
        assert_eq!(
            app.world().resource::<Panel>().feedback,
            Some("Split confirmed")
        );
        let snapshot = authority.snapshot(OWN, |id| id == OWN).unwrap();
        assert_eq!(snapshot.items.len(), 3);
        assert!(snapshot.items.iter().any(|(id, stack)| *id != item
            && stack.placement == placement(0, 1)
            && stack.quantity == 2));
        assert!(transfer(&mut app, &mut server).is_none());
    }

    #[test]
    fn inventory_grid_mouse_merge_previews_and_confirms_partial_remainder() {
        let (mut app, mut authority, _) = fixture_with_stacks(true);
        mouse(&mut app, Vec2::splat(-0.375), true);
        mouse(&mut app, Vec2::new(0.125, -0.375), true);
        assert!(
            app.world_mut()
                .query_filtered::<&Text, With<Feedback>>()
                .single(app.world())
                .unwrap()
                .contains("source remainder ×2")
        );
        mouse(&mut app, Vec2::new(0.125, -0.375), false);
        let mut server = RenetServer::new(data::renet_config());
        server.add_connection(7, false);
        let Some(InventoryRequest::Operate(op)) = transfer(&mut app, &mut server) else {
            panic!("expected Merge");
        };
        assert!(matches!(op.action, InventoryAction::Merge { .. }));
        apply_stack_operation(&mut app, &mut authority, op);
        assert_eq!(
            app.world().resource::<Panel>().feedback,
            Some("Merge confirmed: remainder in source stack")
        );
        let snapshot = authority.snapshot(OWN, |id| id == OWN).unwrap();
        assert_eq!(
            snapshot
                .items
                .iter()
                .map(|(_, item)| item.quantity)
                .collect::<Vec<_>>(),
            [2, 3]
        );
    }
}
