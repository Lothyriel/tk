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
}

#[derive(Clone, Copy)]
pub(super) struct Pending {
    session: SessionKey,
    version: VersionedCompartment,
    operation: u64,
    item: ItemInstanceId,
    target: Placement,
}

#[derive(SystemParam)]
pub(in crate::input) struct PointerInput<'w, 's> {
    buttons: Option<Res<'w, ButtonInput<MouseButton>>>,
    cursor: Query<'w, 's, &'static RelativeCursorPosition, With<Board>>,
}

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

fn check(
    grid: &InventorySnapshot,
    item: ItemInstanceId,
    target: Placement,
) -> Option<Result<(), PlacementError>> {
    let (_, source) = grid.items.iter().find(|(id, _)| *id == item)?;
    Some(
        target.area(footprint(source)).validate(
            Footprint::new(grid.size[0], grid.size[1])?,
            grid.items
                .iter()
                .filter(|(id, _)| *id != item)
                .map(|(_, other)| other.placement.area(footprint(other))),
        ),
    )
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
            if grid.is_some_and(|grid| {
                grid.version.revision > pending.version.revision
                    && grid
                        .items
                        .iter()
                        .any(|(id, item)| *id == pending.item && item.placement == pending.target)
            }) {
                panel.pending = None;
                panel.feedback = Some("Move confirmed");
            } else if grid.is_some_and(|grid| grid.version.revision > pending.version.revision) {
                panel.pending = None;
                panel.feedback = Some("Inventory changed after move");
            }
        }
        Some(OperationResult::Conflict | OperationResult::Expired) => {
            panel.pending = None;
            panel.feedback = Some("Move rejected: inventory changed");
        }
        Some(_) => {
            panel.pending = None;
            panel.feedback = Some("Move rejected by server");
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
            cell.and_then(|[x, y]| {
                grid.items.iter().find(|(_, item)| {
                    let [w, h] = dimensions(item, item.placement.rotation);
                    x >= item.placement.x
                        && x - item.placement.x < w
                        && y >= item.placement.y
                        && y - item.placement.y < h
                })
            })
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
            });
            return true;
        }
        return false;
    }
    let drag = panel.drag.as_mut().unwrap();
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
    let submit = if drag.mouse {
        mouse.is_some_and(|buttons| buttons.just_released(MouseButton::Left))
    } else {
        keys.just_pressed(KeyCode::Enter)
    };
    if !submit {
        return true;
    }
    let drag = panel.drag.take().unwrap();
    let Some(target) = drag.target else {
        panel.feedback = Some("Move cancelled outside grid");
        return true;
    };
    let verdict = check(grid, drag.item, target);
    if verdict != Some(Ok(())) {
        panel.feedback = Some(reason(verdict));
        return true;
    }
    if grid
        .items
        .iter()
        .any(|(id, item)| *id == drag.item && item.placement == target)
    {
        return true;
    }
    let id = grid.version.id;
    let intent = Operate {
        action: InventoryAction::Move {
            item: drag.item,
            destination: GridLocation {
                compartment: id,
                placement: target,
            },
        },
        scope: [id; 2],
    };
    if let Some(operation) = inventory.send_operation(client, &intent) {
        panel.pending = Some(Pending {
            session,
            version: drag.version,
            operation,
            item: drag.item,
            target,
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
                Text::new("BACKPACK GRID\nDrag or Enter to move · R rotate · Esc cancel"),
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
);

type BoardNode<'w, 's> =
    Single<'w, 's, (Entity, &'static mut Node), (With<Board>, Without<GridRoot>)>;

#[allow(clippy::too_many_arguments)]
pub(in crate::input) fn present(
    mut commands: Commands,
    (panel, inventory): (Res<Panel>, Res<State>),
    (session, player): (Res<crate::sync::ClientSession>, Res<common::PlayerId>),
    mut root: Single<&mut Node, With<GridRoot>>,
    mut board: BoardNode,
    mut message: Single<&mut Text, With<Feedback>>,
    tiles: Query<Entity, With<Tile>>,
    mut last: Local<Option<DrawKey>>,
) {
    let grid = inventory
        .owned_grid(session.raid.raid_id, player.0)
        .filter(known_grid);
    let visible = panel.open && panel.inspection.is_none();
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
    let ghost = panel
        .drag
        .and_then(|drag| drag.target.map(|target| (drag.item, target, false)))
        .or_else(|| panel.pending.map(|p| (p.item, p.target, true)));
    let verdict = ghost.and_then(|(id, target, _)| check(grid, id, target));
    let label = if panel.pending.is_some() {
        "Waiting for server confirmation"
    } else if panel.drag.is_some() {
        reason(verdict)
    } else {
        panel.feedback.unwrap_or("Select an item to move")
    };
    if message.as_str() != label {
        ***message = label.into();
    }
    let draw = (key, grid.version, panel.selected, ghost);
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
                } else if verdict == Some(Ok(())) {
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
        let mut source = Inventory::default();
        source
            .register_compartment(OWN, Footprint::new(4, 4).unwrap(), None)
            .unwrap();
        let mut seed = |definition, placement| {
            source
                .insert_seed(
                    definition,
                    Condition::Intact,
                    1,
                    GridLocation {
                        compartment: OWN,
                        placement,
                    },
                )
                .unwrap()
        };
        let movable = seed("food.water500", placement(0, 0));
        seed("med.bandage", placement(2, 0));
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
}
