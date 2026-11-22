use super::{InventoryCue, Panel};
use crate::sync::inventory::{Operate, State};
use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    ui::{FocusPolicy, RelativeCursorPosition, UiScale},
    window::PrimaryWindow,
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

impl Drag {
    pub(super) fn choosing_quantity(self) -> bool {
        self.split.is_some_and(|split| split.choosing)
    }
}

#[derive(Clone, Copy)]
struct Split {
    quantity: u16,
    maximum: u16,
    choosing: bool,
    focus: usize,
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

#[derive(Clone, Copy)]
pub(super) struct Context {
    session: SessionKey,
    version: VersionedCompartment,
    item: ItemInstanceId,
    focus: usize,
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum ContextAction {
    Inspect,
    Drop,
    Close,
}

const CONTEXT_ACTIONS: [(ContextAction, &str); 3] = [
    (ContextAction::Inspect, "Inspect"),
    (ContextAction::Drop, "Drop"),
    (ContextAction::Close, "Close"),
];

pub(super) mod close_ack;

#[derive(SystemParam)]
pub(in crate::input) struct PointerInput<'w, 's> {
    buttons: Option<Res<'w, ButtonInput<MouseButton>>>,
    cursor: Query<'w, 's, &'static RelativeCursorPosition, With<Board>>,
    quantity: Query<'w, 's, (&'static QuantityButton, &'static RelativeCursorPosition)>,
    context: Query<'w, 's, (&'static ContextAction, &'static RelativeCursorPosition)>,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::input) enum QuantityButton {
    Less,
    More,
    Confirm,
    Cancel,
}

const QUANTITY_BUTTONS: [(QuantityButton, &str); 4] = [
    (QuantityButton::Less, "−"),
    (QuantityButton::More, "+"),
    (QuantityButton::Confirm, "Choose"),
    (QuantityButton::Cancel, "Cancel"),
];
const QUANTITY_DEFAULT_FOCUS: usize = 2;
const QUANTITY_FOCUS_COLOR: Color = Color::srgb(0.78, 0.89, 1.0);

#[derive(Component)]
pub(in crate::input) struct QuantityControls;
#[derive(Component)]
pub(in crate::input) struct ContextControls;
#[derive(Component)]
pub(in crate::input) struct ContextLabel(ContextAction);

#[derive(Component)]
pub(in crate::input) struct Board;

#[derive(Component)]
pub(in crate::input) struct GridRoot;

#[derive(Component)]
pub(in crate::input) struct Feedback;

#[derive(Component)]
pub(in crate::input) struct Tile;

#[derive(Component)]
pub(in crate::input) struct ItemTooltip;

mod review;
pub(crate) use review::{GridReview, PointerRoute};

#[derive(Clone, Copy, Component)]
enum GhostKind {
    Valid,
    Invalid,
    Pending,
}

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
    if let Some(context) = panel.context
        && (session != Some(context.session)
            || grid.is_none_or(|grid| grid.version != context.version))
    {
        panel.context = None;
        panel.feedback = Some("Item menu closed: inventory changed");
    }
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
                if confirmed.is_some() {
                    panel.record_cue(InventoryCue::Confirmed);
                }
                panel.feedback = Some(confirmed.unwrap_or("Inventory changed after operation"));
            }
        }
        Some(OperationResult::Conflict | OperationResult::Expired) => {
            panel.pending = None;
            panel.record_cue(InventoryCue::Rejected);
            panel.feedback = Some(if matches!(pending.expected, Expected::Move) {
                "Move rejected: inventory changed"
            } else {
                "Stack operation rejected: inventory changed"
            });
        }
        Some(_) => {
            panel.pending = None;
            panel.record_cue(InventoryCue::Rejected);
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
    if let Some(context) = panel.context.as_mut() {
        if panel.drag.is_some_and(|drag| drag.mouse)
            && mouse.is_some_and(|buttons| !buttons.pressed(MouseButton::Left))
        {
            panel.drag = None;
        }
        if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::Tab) {
            context.focus = (context.focus + 1) % CONTEXT_ACTIONS.len();
        } else if keys.just_pressed(KeyCode::ArrowUp) {
            context.focus = (context.focus + CONTEXT_ACTIONS.len() - 1) % CONTEXT_ACTIONS.len();
        }
        let action = if mouse.is_some_and(|b| b.just_pressed(MouseButton::Left)) {
            pointer
                .context
                .iter()
                .find_map(|(action, cursor)| cursor.cursor_over.then_some(*action))
                .or(Some(ContextAction::Close))
        } else if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) {
            Some(CONTEXT_ACTIONS[context.focus].0)
        } else {
            None
        };
        if let Some(action) = action {
            let item = context.item;
            panel.context = None;
            match action {
                ContextAction::Inspect => {
                    panel.drag = None;
                    panel.inspect(item);
                }
                ContextAction::Drop => {
                    panel.drag = None;
                    panel.request_drop(inventory, client, raid, owner, item);
                }
                ContextAction::Close => (),
            }
        }
        return true;
    }
    let right_click = mouse.is_some_and(|buttons| buttons.just_pressed(MouseButton::Right));
    let menu_key = keys.just_pressed(KeyCode::ContextMenu) || keys.just_pressed(KeyCode::F10);
    if (right_click || menu_key) && panel.drag.is_none_or(|drag| drag.split.is_none()) {
        let selected = if right_click {
            cell.and_then(|cell| item_at(grid, cell)).map(|(id, _)| *id)
        } else {
            panel
                .selected
                .filter(|selected| grid.items.iter().any(|(id, _)| id == selected))
        };
        if let Some(item) = selected {
            panel.selected = Some(item);
            panel.context = Some(Context {
                session,
                version: grid.version,
                item,
                focus: 0,
            });
        }
        return true;
    }
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
                    focus: QUANTITY_DEFAULT_FOCUS,
                }),
                hover: None,
            });
            return true;
        }
        return false;
    }
    let drag = panel.drag.as_mut().unwrap();
    if let Some(split) = drag.split.as_mut().filter(|s| s.choosing) {
        if keys.just_pressed(KeyCode::Tab) {
            let backward = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
            let len = QUANTITY_BUTTONS.len();
            split.focus = (split.focus + if backward { len - 1 } else { 1 }) % len;
            return true;
        }
        let button = mouse
            .is_some_and(|b| b.just_pressed(MouseButton::Left))
            .then(|| {
                pointer
                    .quantity
                    .iter()
                    .find_map(|(button, cursor)| cursor.cursor_over.then_some(*button))
            })
            .flatten()
            .or_else(|| {
                keys.just_pressed(KeyCode::Enter)
                    .then(|| QUANTITY_BUTTONS[split.focus].0)
            });
        if matches!(button, Some(QuantityButton::Cancel)) {
            panel.drag = None;
            panel.feedback = Some("Split cancelled");
        } else if matches!(button, Some(QuantityButton::Confirm)) {
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
        // The mouse may have been released while a context menu covered the drag.
        if drag.mouse && mouse.is_some_and(|buttons| !buttons.pressed(MouseButton::Left)) {
            panel.drag = None;
        }
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

const GRID_FONT_BYTES: &[u8] = include_bytes!("../../../../assets/fonts/FiraMono-Medium.ttf");

#[derive(Resource)]
pub(crate) struct GridFont(Handle<Font>);

impl GridFont {
    pub(crate) fn sized(&self, size: f32) -> TextFont {
        TextFont::from_font_size(size).with_font(self.0.clone())
    }
}

fn reason(verdict: Option<Result<(), PlacementError>>) -> &'static str {
    match verdict {
        Some(Ok(())) => "Valid placement",
        Some(Err(PlacementError::Occupied)) => "Cell occupied",
        Some(Err(PlacementError::OutsideGrid)) => "Item exceeds grid edge",
        None => "Outside grid",
    }
}

pub(in crate::input) fn setup(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let font = GridFont(fonts.add(Font::from_bytes(GRID_FONT_BYTES.to_vec())));
    commands.spawn((
        ItemTooltip,
        Pickable::IGNORE,
        FocusPolicy::Pass,
        GlobalZIndex(230),
        Text::default(),
        font.sized(13.0),
        BackgroundColor(Color::srgb(0.04, 0.08, 0.12)),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            padding: UiRect::all(Val::Px(6.0)),
            overflow: Overflow::clip(),
            ..default()
        },
    ));
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
                font.sized(13.0),
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
            parent.spawn((Feedback, Text::default(), font.sized(14.0)));
            parent.spawn((QuantityControls, Node { display: Display::None, column_gap: Val::Px(5.0), ..default() }))
                .with_children(|buttons| {
                    for (action, label) in QUANTITY_BUTTONS {
                        buttons.spawn((
                            action, RelativeCursorPosition::default(),
                            Node { padding: UiRect::all(Val::Px(7.0)), ..default() },
                            BackgroundColor(Color::srgb(0.17, 0.29, 0.40)),
                            Outline::new(Val::Px(2.0), Val::Px(-2.0), Color::NONE),
                        )).with_child((FocusPolicy::Pass, Text::new(label), font.sized(14.0)));
                    }
                });
            parent.spawn((ContextControls, Node { display: Display::None, column_gap: Val::Px(5.0), ..default() }))
                .with_children(|buttons| {
                    for (action, label) in CONTEXT_ACTIONS {
                        buttons.spawn((
                            action, RelativeCursorPosition::default(),
                            Node { padding: UiRect::all(Val::Px(7.0)), ..default() },
                            BackgroundColor(Color::srgb(0.17, 0.29, 0.40)),
                        )).with_child((ContextLabel(action), FocusPolicy::Pass, Text::new(label), font.sized(14.0)));
                    }
                });
        });
    commands.insert_resource(font);
}

fn tooltip_rect(pointer: Vec2, viewport: Vec2) -> (Vec2, Vec2) {
    let size = Vec2::new(212.0, 64.0).min(viewport);
    let next = pointer + Vec2::splat(12.0);
    let before = pointer - size - Vec2::splat(12.0);
    let origin = Vec2::new(
        if next.x + size.x > viewport.x {
            before.x
        } else {
            next.x
        },
        if next.y + size.y > viewport.y {
            before.y
        } else {
            next.y
        },
    )
    .clamp(Vec2::ZERO, viewport - size);
    (origin, size)
}

pub(in crate::input) fn present_tooltip(
    (panel, inventory): (Res<Panel>, Res<State>),
    (session, player): (Res<crate::sync::ClientSession>, Res<common::PlayerId>),
    window: Single<&Window, With<PrimaryWindow>>,
    scale: Option<Res<UiScale>>,
    board: Single<&RelativeCursorPosition, With<Board>>,
    mut tooltip: Single<(&mut Node, &mut Text), With<ItemTooltip>>,
) {
    let show = || {
        if !panel.open
            || panel.inspection.is_some()
            || panel.context.is_some()
            || panel.drag.is_some()
            || panel.pending.is_some()
        {
            return None;
        }
        let grid = inventory
            .owned_grid(session.raid.raid_id, player.0)
            .filter(known_grid)?;
        let (_, item) = item_at(grid, cell_at(&board, grid.size)?)?;
        let pointer = window.cursor_position()?;
        let scale = scale.as_ref().map_or(1.0, |scale| scale.0).max(0.01);
        let viewport = Vec2::new(window.width(), window.height()) / scale;
        let (origin, size) = tooltip_rect(pointer / scale, viewport);
        let name = planned_items()[item.definition.as_str()].name;
        Some((
            origin,
            size,
            format!("{name}\nQuantity: {} · {:?}", item.quantity, item.condition),
        ))
    };
    if let Some((origin, size, text)) = show() {
        let (node, label) = &mut *tooltip;
        node.display = Display::Flex;
        node.left = Val::Px(origin.x);
        node.top = Val::Px(origin.y);
        node.width = Val::Px(size.x);
        node.height = Val::Px(size.y);
        if label.as_str() != text {
            **label = Text::new(text);
        }
    } else {
        tooltip.0.display = Display::None;
    }
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
type QuantityNode<'w, 's> = Single<
    'w,
    's,
    &'static mut Node,
    (
        With<QuantityControls>,
        Without<GridRoot>,
        Without<Board>,
        Without<ContextControls>,
    ),
>;
type ContextNode<'w, 's> =
    Single<'w, 's, &'static mut Node, (With<ContextControls>, Without<GridRoot>, Without<Board>)>;
type Viewport<'w, 's> = (
    Option<Single<'w, 's, &'static Window, With<PrimaryWindow>>>,
    Option<Res<'w, UiScale>>,
);

fn board_side(view_height: f32) -> f32 {
    // In UI units, reserve room for the top offset, both paddings, the
    // wrapped header/feedback and one action row. Keep the size independent
    // of the current action so opening a chooser never moves the cells.
    (view_height - 250.0).clamp(1.0, 276.0)
}

#[allow(clippy::too_many_arguments)]
pub(in crate::input) fn present(
    mut commands: Commands,
    font: Res<GridFont>,
    (panel, inventory): (Res<Panel>, Res<State>),
    (session, player): (Res<crate::sync::ClientSession>, Res<common::PlayerId>),
    (window, scale): Viewport,
    mut root: Single<&mut Node, With<GridRoot>>,
    mut board: BoardNode,
    mut quantity: QuantityNode,
    mut message: Single<&mut Text, (With<Feedback>, Without<ContextLabel>)>,
    mut context_node: ContextNode,
    mut context_labels: Query<(&ContextLabel, &mut Text), Without<Feedback>>,
    mut quantity_outlines: Query<(&QuantityButton, &mut Outline)>,
    tiles: Query<Entity, With<Tile>>,
    mut last: Local<Option<DrawKey>>,
) {
    let grid = inventory
        .owned_grid(session.raid.raid_id, player.0)
        .filter(known_grid);
    let visible = panel.open && panel.inspection.is_none();
    context_node.display = if visible && panel.context.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    if let Some(context) = panel.context {
        for (action, mut text) in &mut context_labels {
            let (index, (_, label)) = CONTEXT_ACTIONS
                .iter()
                .enumerate()
                .find(|(_, (candidate, _))| *candidate == action.0)
                .unwrap();
            let label = if index == context.focus {
                format!("› {label}")
            } else {
                (*label).to_owned()
            };
            if text.as_str() != label {
                **text = label;
            }
        }
    }
    let choosing = panel.drag.and_then(|d| d.split).filter(|s| s.choosing);
    if visible {
        let focused = choosing.map(|split| QUANTITY_BUTTONS[split.focus].0);
        for (button, mut outline) in &mut quantity_outlines {
            let color = if Some(*button) == focused {
                QUANTITY_FOCUS_COLOR
            } else {
                Color::NONE
            };
            if outline.color != color {
                outline.color = color;
            }
        }
    }
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
    if let Some(window) = window {
        let scale = scale.as_ref().map_or(1.0, |scale| scale.0);
        let width = Val::Px(board_side(window.height() / scale));
        if board.1.width != width {
            board.1.width = width;
        }
    }
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
    let label = if let Some(context) = panel.context {
        grid.items
            .iter()
            .find(|(id, _)| *id == context.item)
            .map_or_else(
                || "Item menu".to_owned(),
                |(_, item)| {
                    format!(
                        "{} ×{} · arrows / Enter · Esc",
                        planned_items()[item.definition.as_str()].name,
                        item.quantity
                    )
                },
            )
    } else if let Some(split) = choosing {
        format!(
            "Split {} / {} · arrows adjust · Tab focus · Enter activate",
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
                    font.sized(12.0),
                    TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
                ));
        }
        if let Some((id, target, pending)) = ghost
            && let Some((_, item)) = grid.items.iter().find(|(other, _)| *other == id)
        {
            board.spawn((
                Tile,
                if pending {
                    GhostKind::Pending
                } else if valid {
                    GhostKind::Valid
                } else {
                    GhostKind::Invalid
                },
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

    #[test]
    fn inventory_grid_tooltip_clamps_at_all_edges_and_large_scale() {
        for viewport in [
            Vec2::new(1280., 720.),
            Vec2::new(640., 360.),
            Vec2::new(130., 44.),
        ] {
            for pointer in [
                Vec2::ZERO,
                viewport,
                Vec2::new(0., viewport.y),
                Vec2::new(viewport.x, 0.),
            ] {
                let (origin, size) = tooltip_rect(pointer, viewport);
                assert!(origin.cmpge(Vec2::ZERO).all());
                assert!((origin + size).cmple(viewport).all());
                assert_eq!(size, Vec2::new(212., 64.).min(viewport));
            }
        }
        assert_eq!(
            tooltip_rect(Vec2::ZERO, Vec2::splat(500.)).0,
            Vec2::splat(12.)
        );
        assert_eq!(
            tooltip_rect(Vec2::splat(500.), Vec2::splat(500.)).0,
            Vec2::new(276., 424.)
        );
    }

    const KEY: SessionKey = SessionKey {
        raid_epoch: 1,
        session_epoch: 1,
    };
    const OWN: CompartmentId = CompartmentId(7);

    fn fixture() -> (App, InventoryAuthority, ItemInstanceId) {
        fixture_with_stacks(false)
    }

    #[test]
    fn inventory_grid_uses_licensed_face_and_preserves_word_wrapped_names() {
        use bevy::text::{FontSource, LineBreak};
        let (mut app, _, _) = fixture();
        let world = app.world_mut();
        let handle = world.resource::<GridFont>().0.clone();
        assert_eq!(
            world
                .resource::<Assets<Font>>()
                .get(&handle)
                .unwrap()
                .data
                .as_ref(),
            GRID_FONT_BYTES
        );
        let mut labels = world.query::<(&Text, &TextFont, &TextLayout)>();
        let mut items = Vec::new();
        for (label, font, layout) in labels.iter(world) {
            assert_eq!(font.font, FontSource::Handle(handle.clone()));
            if label.contains('×') {
                assert_eq!(layout.linebreak, LineBreak::WordOrCharacter);
                items.push(label.as_str());
            }
        }
        assert!(items.contains(&"Água potável 500 ml ×1"));
        assert!(items.contains(&"Bandagem estéril ×1"));
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
            .init_resource::<Assets<Font>>()
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
    fn inventory_grid_pending_is_not_replayed_after_panel_exit() {
        enum Exit {
            Tab,
            Disconnect,
            Unfocus,
        }
        for exit in [Exit::Tab, Exit::Disconnect, Exit::Unfocus] {
            let (mut app, mut authority, item) = fixture();
            key(&mut app, KeyCode::Enter);
            key(&mut app, KeyCode::ArrowDown);
            key(&mut app, KeyCode::Enter);
            let mut server = RenetServer::new(data::renet_config());
            server.add_connection(7, false);
            let Some(InventoryRequest::Operate(op)) = transfer(&mut app, &mut server) else {
                panic!("expected exactly one submitted move");
            };
            assert!(app.world().resource::<Panel>().pending.is_some());

            let revision = app
                .world()
                .resource::<State>()
                .owned_grid(1, 7)
                .unwrap()
                .version;
            let mut windows = app
                .world_mut()
                .query_filtered::<&mut Window, With<PrimaryWindow>>();
            match exit {
                Exit::Tab => key(&mut app, KeyCode::Tab),
                Exit::Disconnect => {
                    app.world_mut().resource_mut::<RenetClient>().disconnect();
                    key(&mut app, KeyCode::Tab);
                }
                Exit::Unfocus => {
                    app.world_mut()
                        .resource_mut::<ButtonInput<KeyCode>>()
                        .reset_all();
                    windows.single_mut(app.world_mut()).unwrap().focused = false;
                    app.update();
                    assert_eq!(
                        app.world()
                            .resource::<State>()
                            .owned_grid(1, 7)
                            .unwrap()
                            .version,
                        revision
                    );
                }
            }
            let panel = app.world().resource::<Panel>();
            assert!(!panel.open && panel.pending.is_none() && panel.drag.is_none());
            assert!(panel.feedback.is_none() && panel.cue.is_none());
            assert!(transfer(&mut app, &mut server).is_none());

            if !matches!(exit, Exit::Disconnect) {
                // Closing the UI cannot retract a move already sent to the
                // authority. A later reply must not restore the old ghost.
                let reply = authority.apply(&mut OperationSession::new(KEY), op, |id| id == OWN);
                assert_eq!(reply.result, OperationResult::Committed);
                for body in [
                    UpdateBody::Result {
                        id: op.id,
                        result: reply.result,
                    },
                    UpdateBody::Snapshot(authority.snapshot(OWN, |id| id == OWN).unwrap()),
                ] {
                    app.world_mut()
                        .resource_mut::<State>()
                        .apply(InventoryUpdate { session: KEY, body });
                }
                if matches!(exit, Exit::Unfocus) {
                    app.update();
                    windows.single_mut(app.world_mut()).unwrap().focused = true;
                    app.update();
                    assert!(!app.world().resource::<Panel>().open);
                }
                key(&mut app, KeyCode::Tab);
                let panel = app.world().resource::<Panel>();
                assert!(panel.open && panel.pending.is_none() && panel.drag.is_none());
                assert!(panel.cue.is_none());
                let grid = app.world().resource::<State>().owned_grid(1, 7).unwrap();
                let (_, moved) = grid.items.iter().find(|(id, _)| *id == item).unwrap();
                assert_eq!(moved.placement, placement(0, 1));
                assert!(transfer(&mut app, &mut server).is_none());
            }
        }
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
        assert_eq!(app.world_mut().resource_mut::<Panel>().take_cue(), None);
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
        assert_eq!(
            app.world_mut().resource_mut::<Panel>().take_cue(),
            Some(InventoryCue::Confirmed)
        );
        app.update();
        assert_eq!(app.world_mut().resource_mut::<Panel>().take_cue(), None);
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
    fn inventory_grid_rejects_invalid_moves_and_resyncs_after_authority_conflict() {
        let (mut app, mut authority, item) = fixture();
        crate::sync::add_inventory_test_receive(&mut app);
        let mut server = RenetServer::new(data::renet_config());
        server.add_connection(7, false);
        let original = authority.snapshot(OWN, |id| id == OWN).unwrap();
        let assert_confirmed = |app: &App, expected: &InventorySnapshot| {
            assert_eq!(
                app.world().resource::<State>().owned_grid(1, 7),
                Some(expected)
            );
        };
        for (keys, reason) in [
            (&[KeyCode::KeyR, KeyCode::ArrowRight][..], "Cell occupied"),
            (&[KeyCode::ArrowDown; 3][..], "Item exceeds grid edge"),
        ] {
            key(&mut app, KeyCode::Enter);
            for &press in keys {
                key(&mut app, press);
            }
            let world = app.world_mut();
            let mut labels = world.query_filtered::<&Text, With<Feedback>>();
            assert_eq!(labels.single(world).unwrap().as_str(), reason);
            assert_confirmed(&app, &original);
            key(&mut app, KeyCode::Enter);
            let panel = app.world().resource::<Panel>();
            assert!(panel.drag.is_none() && panel.pending.is_none());
            assert_eq!(panel.feedback, Some(reason));
            assert_confirmed(&app, &original);
            assert!(transfer(&mut app, &mut server).is_none());
        }

        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::ArrowDown);
        key(&mut app, KeyCode::Enter);
        let Some(InventoryRequest::Operate(op)) = transfer(&mut app, &mut server) else {
            panic!("expected move");
        };
        let panel = app.world().resource::<Panel>();
        assert!(panel.pending.is_some());
        assert_confirmed(&app, &original);
        assert_eq!(op.expected, [original.version; 2]);
        let other_item = original.items.iter().find(|(id, _)| *id != item).unwrap().0;
        // Advance authority while the view is stale. Move only the bandage,
        // so the attempted water move is still geometrically legal.
        let mut writer = OperationSession::new(KEY);
        let change = authority.apply(
            &mut writer,
            common::catalog::item_inventory::InventoryOperation {
                action: InventoryAction::Move {
                    item: other_item,
                    destination: GridLocation {
                        compartment: OWN,
                        placement: placement(3, 0),
                    },
                },
                ..op
            },
            |id| id == OWN,
        );
        assert_eq!(change.result, OperationResult::Committed);
        let latest = authority.snapshot(OWN, |id| id == OWN).unwrap();
        assert_eq!(latest.version.revision, original.version.revision + 1);
        let reply = authority.apply(&mut OperationSession::new(KEY), op, |id| id == OWN);
        assert_eq!(reply.result, OperationResult::Conflict);
        assert!(reply.commit.is_none());
        assert_eq!(authority.snapshot(OWN, |id| id == OWN).unwrap(), latest);
        let deliver = |app: &mut App, server: &mut RenetServer, body| {
            server.send_message(
                7,
                CHANNEL,
                InventoryUpdate { session: KEY, body }.encode().unwrap(),
            );
            for packet in server.get_packets_to_send(7).unwrap() {
                app.world_mut()
                    .resource_mut::<RenetClient>()
                    .process_packet(packet);
            }
            app.update();
        };
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        deliver(
            &mut app,
            &mut server,
            UpdateBody::Result {
                id: op.id,
                result: reply.result,
            },
        );
        let panel = app.world().resource::<Panel>();
        assert!(panel.pending.is_none());
        assert_eq!(panel.feedback, Some("Move rejected: inventory changed"));
        assert_eq!(
            app.world_mut().resource_mut::<Panel>().take_cue(),
            Some(InventoryCue::Rejected)
        );
        assert!(app.world().resource::<State>().owned_grid(1, 7).is_none());
        let Some(InventoryRequest::Resync {
            session,
            compartment,
        }) = transfer(&mut app, &mut server)
        else {
            panic!("expected private grid resync");
        };
        assert_eq!((session, compartment), (KEY, OWN));
        assert!(transfer(&mut app, &mut server).is_none());
        let water = latest.items.iter().find(|(id, _)| *id == item).unwrap();
        assert_eq!(
            original.items.iter().find(|(id, _)| *id == item),
            Some(water)
        );
        deliver(
            &mut app,
            &mut server,
            UpdateBody::Snapshot(authority.snapshot(compartment, |id| id == OWN).unwrap()),
        );
        assert_confirmed(&app, &latest);
        assert_eq!(app.world_mut().resource_mut::<Panel>().take_cue(), None);
        assert!(transfer(&mut app, &mut server).is_none());
    }

    #[test]
    fn inventory_grid_escape_closes_context_then_drag_then_panel() {
        let (mut app, _, _) = fixture();
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::ContextMenu);
        assert!(app.world().resource::<Panel>().context.is_some());
        for (open, drag) in [(true, true), (true, false), (false, false)] {
            key(&mut app, KeyCode::Escape);
            let panel = app.world().resource::<Panel>();
            assert!(panel.context.is_none());
            assert!(panel.consumed_escape);
            assert_eq!((panel.open, panel.drag.is_some()), (open, drag));
        }
    }

    #[test]
    fn inventory_grid_context_drop_targets_the_displayed_revision() {
        let (mut app, authority, item) = fixture();
        key(&mut app, KeyCode::ContextMenu);
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Enter);
        let mut server = RenetServer::new(data::renet_config());
        server.add_connection(7, false);
        let snapshot = authority.snapshot(OWN, |id| id == OWN).unwrap();
        assert_eq!(
            transfer(&mut app, &mut server),
            Some(InventoryRequest::Drop {
                session: KEY,
                revision: snapshot.version.revision,
                item,
            })
        );
        assert_eq!(
            app.world().resource::<Panel>().feedback,
            Some("Drop requested; waiting for inventory update")
        );
        assert_eq!(app.world_mut().resource_mut::<Panel>().take_cue(), None);
        assert!(
            app.world()
                .resource::<State>()
                .owned_grid(1, 7)
                .unwrap()
                .items
                .iter()
                .any(|(id, _)| *id == item)
        );

        key(&mut app, KeyCode::ContextMenu);
        key(&mut app, KeyCode::ArrowDown);
        let mut changed = authority.snapshot(OWN, |id| id == OWN).unwrap();
        changed.version.revision += 1;
        app.world_mut()
            .resource_mut::<State>()
            .apply(InventoryUpdate {
                session: KEY,
                body: UpdateBody::Snapshot(changed),
            });
        key(&mut app, KeyCode::Enter);
        assert!(transfer(&mut app, &mut server).is_none());
        assert!(app.world().resource::<Panel>().context.is_none());
    }

    #[test]
    fn inventory_grid_mouse_context_inspects_without_sending_an_operation() {
        let (mut app, _, selected) = fixture();
        let board = app
            .world_mut()
            .query_filtered::<Entity, With<Board>>()
            .single(app.world())
            .unwrap();
        *app.world_mut()
            .get_mut::<RelativeCursorPosition>(board)
            .unwrap() = RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::splat(-0.375)),
        };
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        app.update();
        assert_eq!(
            app.world().resource::<Panel>().context.unwrap().item,
            selected
        );

        let inspect = app
            .world_mut()
            .query::<(Entity, &ContextAction)>()
            .iter(app.world())
            .find_map(|(entity, action)| (*action == ContextAction::Inspect).then_some(entity))
            .unwrap();
        app.world_mut()
            .get_mut::<RelativeCursorPosition>(inspect)
            .unwrap()
            .cursor_over = true;
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.clear();
        mouse.press(MouseButton::Left);
        app.update();
        let panel = app.world().resource::<Panel>();
        assert!(panel.context.is_none());
        assert_eq!(panel.inspection.unwrap().item, selected);
        let mut server = RenetServer::new(data::renet_config());
        server.add_connection(7, false);
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
    fn inventory_grid_reserves_action_height_without_changing_standard_layout() {
        assert_eq!(board_side(720.0), 276.0);
        assert_eq!(board_side(720.0 / 1.25), 276.0);
        assert_eq!(board_side(720.0 / 1.5), 230.0);
        assert_eq!(board_side(800.0 / 1.5), 276.0);
    }

    #[test]
    fn inventory_grid_escape_cancels_split_before_or_after_quantity_choice() {
        for choose_quantity in [false, true] {
            let (mut app, _, _) = fixture_with_stacks(true);
            let revision = app
                .world()
                .resource::<State>()
                .owned_grid(1, 7)
                .unwrap()
                .version;
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::ShiftLeft);
            keys.press(KeyCode::Enter);
            app.update();
            if choose_quantity {
                key(&mut app, KeyCode::Enter);
                key(&mut app, KeyCode::ArrowDown);
            }
            let panel = app.world().resource::<Panel>();
            assert_eq!(
                panel.drag.unwrap().split.unwrap().choosing,
                !choose_quantity
            );
            key(&mut app, KeyCode::Escape);
            let panel = app.world().resource::<Panel>();
            assert!(panel.open && panel.drag.is_none() && panel.pending.is_none());
            assert_eq!(
                app.world()
                    .resource::<State>()
                    .owned_grid(1, 7)
                    .unwrap()
                    .version,
                revision
            );
            let mut server = RenetServer::new(data::renet_config());
            server.add_connection(7, false);
            assert!(transfer(&mut app, &mut server).is_none());
            key(&mut app, KeyCode::Escape);
            assert!(!app.world().resource::<Panel>().open);
        }
    }

    #[test]
    fn inventory_grid_split_focus_wraps_and_activates_without_submitting() {
        use QuantityButton::{Cancel, Confirm, Less, More};

        let (mut app, authority, _) = fixture_with_stacks(true);
        let original = authority.snapshot(OWN, |id| id == OWN).unwrap();
        let mut server = RenetServer::new(data::renet_config());
        server.add_connection(7, false);
        let chord = |app: &mut App, modifier, key| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(modifier);
            keys.press(key);
            app.update();
        };
        let assert_focus = |app: &mut App, expected, quantity| {
            let panel = app.world().resource::<Panel>();
            assert!(panel.open && panel.pending.is_none());
            let split = panel.drag.unwrap().split.unwrap();
            assert!(split.choosing);
            assert_eq!(split.quantity, quantity);
            let world = app.world_mut();
            let mut outlines = world.query::<(&QuantityButton, &Outline)>();
            let focused: Vec<_> = outlines
                .iter(world)
                .filter_map(|(button, outline)| (outline.color != Color::NONE).then_some(*button))
                .collect();
            assert_eq!(focused, [expected]);
        };

        chord(&mut app, KeyCode::ShiftLeft, KeyCode::Enter);
        assert_focus(&mut app, Confirm, 1);
        for expected in [Cancel, Less, More, Confirm] {
            key(&mut app, KeyCode::Tab);
            assert_focus(&mut app, expected, 1);
        }
        for expected in [More, Less, Cancel, Confirm, More] {
            chord(&mut app, KeyCode::ShiftRight, KeyCode::Tab);
            assert_focus(&mut app, expected, 1);
        }
        key(&mut app, KeyCode::Enter);
        assert_focus(&mut app, More, 2);
        chord(&mut app, KeyCode::ShiftLeft, KeyCode::Tab);
        key(&mut app, KeyCode::Enter);
        assert_focus(&mut app, Less, 1);
        chord(&mut app, KeyCode::ShiftLeft, KeyCode::Tab);
        key(&mut app, KeyCode::Enter);
        let panel = app.world().resource::<Panel>();
        assert!(panel.open && panel.drag.is_none() && panel.pending.is_none());
        assert_eq!(panel.feedback, Some("Split cancelled"));

        chord(&mut app, KeyCode::ShiftLeft, KeyCode::Enter);
        assert_focus(&mut app, Confirm, 1);
        key(&mut app, KeyCode::Enter);
        let panel = app.world().resource::<Panel>();
        assert!(panel.open && panel.pending.is_none());
        assert!(!panel.drag.unwrap().choosing_quantity());
        assert!(transfer(&mut app, &mut server).is_none());
        assert_eq!(
            app.world().resource::<State>().owned_grid(1, 7),
            Some(&original)
        );

        key(&mut app, KeyCode::Escape);
        let panel = app.world().resource::<Panel>();
        assert!(panel.open && panel.drag.is_none() && panel.consumed_escape);
        key(&mut app, KeyCode::Tab);
        assert!(!app.world().resource::<Panel>().open);
        assert!(transfer(&mut app, &mut server).is_none());
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
