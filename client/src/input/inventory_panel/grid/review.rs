//! Read the live grid after layout; never stage input, inventory, or pending.
use std::{collections::HashSet, path::PathBuf};

use bevy::{
    input::{
        ButtonState, InputSystems,
        keyboard::{Key, KeyboardInput},
        mouse::MouseButtonInput,
    },
    render::view::screenshot::{Capturing, Screenshot, save_to_disk},
    ui::{UiGlobalTransform, UiSystems},
    window::InternalWindowState,
};

use super::*;

pub(crate) struct GridReview {
    pub automated: bool,
    pub pointer_checkpoint: bool,
}

impl Plugin for GridReview {
    fn build(&self, app: &mut App) {
        let directory = PathBuf::from(
            std::env::var_os("TK_GRID_REVIEW_DIR")
                .expect("set TK_GRID_REVIEW_DIR to a new directory"),
        );
        std::fs::create_dir(&directory).expect("create a new grid review directory");
        if let Ok(value) = std::env::var("TK_GRID_REVIEW_UI_SCALE") {
            let scale: f32 = value.parse().expect("numeric grid review UI scale");
            assert!(
                (0.75..=1.5).contains(&scale),
                "grid review scale must be 0.75–1.5"
            );
            app.insert_resource(UiScale(scale));
        }
        app.init_resource::<Observed>()
            .insert_resource(Output {
                directory,
                automated: self.automated,
            })
            .add_systems(PostUpdate, observe.after(UiSystems::PostLayout));
        if self.pointer_checkpoint {
            app.insert_resource(PointerRoute {
                close_context: self.automated,
                ..default()
            })
            .add_systems(
                PreUpdate,
                pointer_checkpoint
                    .before(InputSystems)
                    .before(UiSystems::Focus),
            )
            .add_systems(PostUpdate, restore_pointer.after(observe));
        }
    }
}

#[derive(Resource)]
struct Output {
    directory: PathBuf,
    automated: bool,
}

#[derive(Resource, Default)]
struct Observed(HashSet<&'static str>);

#[derive(Resource, Default)]
pub(crate) struct PointerRoute {
    target: Option<(SessionKey, VersionedCompartment, ItemInstanceId, f64)>,
    pressed: bool,
    released: bool,
    finished: bool,
    close_context: bool,
    escape_pressed: bool,
    saved_window_state: Option<InternalWindowState>,
}

impl PointerRoute {
    pub(crate) fn finished(&self) -> bool {
        self.finished
    }
}

fn restore_pointer(
    mut route: ResMut<PointerRoute>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    if let Some(saved) = route.saved_window_state.take() {
        window.bypass_change_detection().internal = saved;
    }
}

fn cell_center(size: [u8; 2], board_size: Vec2, placement: Placement) -> Vec2 {
    ((Vec2::new(placement.x.into(), placement.y.into()) + Vec2::splat(0.5))
        / Vec2::new(size[0].into(), size[1].into())
        - Vec2::splat(0.5))
        * board_size
}

type PointerState<'w> = (
    Res<'w, Panel>,
    Res<'w, State>,
    Res<'w, crate::sync::ClientSession>,
    Res<'w, common::PlayerId>,
    Res<'w, Time<Real>>,
);

fn pointer_checkpoint(
    mut route: ResMut<PointerRoute>,
    (panel, inventory, session, player, time): PointerState,
    observed: Res<Observed>,
    board: Single<(&ComputedNode, &UiGlobalTransform, &RelativeCursorPosition), With<Board>>,
    mut window: Single<(Entity, &mut Window), With<PrimaryWindow>>,
    (mut mouse, mut keyboard): (
        MessageWriter<MouseButtonInput>,
        MessageWriter<KeyboardInput>,
    ),
) {
    if route.finished {
        return;
    }
    let button = |state| MouseButtonInput {
        button: MouseButton::Right,
        state,
        window: window.0,
    };
    if route.pressed && !route.released {
        mouse.write(button(ButtonState::Released));
        route.released = true;
    }
    let now = time.elapsed_secs_f64();
    if let Some((_, _, _, started)) = route.target {
        assert!(
            now - started < 8.0,
            "GRID POINTER checkpoint timed out before observing hover and context"
        );
    }
    if !window.1.focused || !panel.open || panel.inspection.is_some() || !session.playing(player.0)
    {
        return;
    }
    let Some(grid) = inventory
        .owned_grid(session.raid.raid_id, player.0)
        .filter(known_grid)
    else {
        return;
    };
    let Some(key) = inventory.grid_session() else {
        return;
    };
    let (node, transform, cursor) = *board;
    if node.size().min_element() <= 0.0 {
        return;
    }
    let Some((id, item)) = grid.items.first() else {
        return;
    };
    let (old_key, version, target, _) = *route.target.get_or_insert((key, grid.version, *id, now));
    assert_eq!(
        (key, grid.version, *id),
        (old_key, version, target),
        "inventory changed during GRID POINTER checkpoint"
    );
    let escape = |state| KeyboardInput {
        key_code: KeyCode::Escape,
        logical_key: Key::Escape,
        state,
        text: None,
        repeat: false,
        window: window.0,
    };
    if route.escape_pressed {
        assert!(
            panel.open && panel.context.is_none() && panel.consumed_escape,
            "GRID POINTER Escape must close only the context menu"
        );
        keyboard.write(escape(ButtonState::Released));
        info!("GRID POINTER context closed via Escape; resuming keyboard route");
        route.finished = true;
        return;
    }
    if observed.0.contains("context") {
        assert!(route.pressed && observed.0.contains("hover"));
        assert!(panel.context.is_some_and(|context| context.item == target
            && context.version == version
            && context.session == key));
        info!(
            "GRID POINTER hover/context observed for item={} revision={} (synthetic Bevy input)",
            target.0, version.revision
        );
        if route.close_context {
            keyboard.write(escape(ButtonState::Pressed));
            route.escape_pressed = true;
        } else {
            route.finished = true;
        }
        return;
    }
    if route.close_context && !observed.0.contains("idle") {
        return;
    }
    let point = *transform * cell_center(grid.size, node.size(), item.placement);
    // Winit applies changed cursor positions in Last. Restore the exact
    // backend cache in PostUpdate, after UI focus and tooltip presentation.
    assert!(
        point.cmpge(Vec2::ZERO).all()
            && point.x < window.1.physical_width() as f32
            && point.y < window.1.physical_height() as f32,
        "GRID POINTER cell is outside the window: {point:?}"
    );
    if !route.pressed
        && observed.0.contains("hover")
        && cell_at(cursor, grid.size)
            .and_then(|cell| item_at(grid, cell))
            .is_some_and(|(id, _)| *id == target)
    {
        mouse.write(button(ButtonState::Pressed));
        route.pressed = true;
    }
    route.saved_window_state = Some(window.1.internal);
    window
        .1
        .bypass_change_detection()
        .set_physical_cursor_position(Some(point.as_dvec2()));
}

type Role = (
    Has<GridRoot>,
    Has<Board>,
    Has<ItemTooltip>,
    Has<ContextControls>,
    Has<ContextAction>,
    Has<Feedback>,
);
type ReviewNodes<'w, 's> = Query<
    'w,
    's,
    (
        &'static ComputedNode,
        &'static UiGlobalTransform,
        Role,
        Option<&'static GhostKind>,
    ),
    Or<(
        With<GridRoot>,
        With<Board>,
        With<ItemTooltip>,
        With<ContextControls>,
        With<ContextAction>,
        With<Feedback>,
        With<GhostKind>,
    )>,
>;

fn layout_rect(node: &ComputedNode, transform: &UiGlobalTransform) -> Rect {
    let half = node.size() * 0.5;
    let mut bounds = Rect {
        min: Vec2::splat(f32::INFINITY),
        max: Vec2::splat(f32::NEG_INFINITY),
    };
    for corner in [
        half,
        -half,
        Vec2::new(-half.x, half.y),
        Vec2::new(half.x, -half.y),
    ] {
        let point = *transform * corner;
        bounds.min = bounds.min.min(point);
        bounds.max = bounds.max.max(point);
    }
    bounds
}

fn fits(rect: Rect, viewport: Vec2) -> bool {
    rect.min.is_finite()
        && rect.max.is_finite()
        && rect.min.min_element() >= -1.0
        && (rect.max - viewport).max_element() <= 1.0
}

#[allow(clippy::too_many_arguments)]
fn observe(
    mut commands: Commands,
    (output, window, scale): (
        Res<Output>,
        Single<&Window, With<PrimaryWindow>>,
        Res<UiScale>,
    ),
    (panel, inventory, session, player): (
        Res<Panel>,
        Res<State>,
        Res<crate::sync::ClientSession>,
        Res<common::PlayerId>,
    ),
    nodes: ReviewNodes,
    // A prior frame awaiting GPU readback must not hide a transient state
    // in this frame. Only an unclaimed request competes for this render target.
    screenshots: Query<(), (With<Screenshot>, Without<Capturing>)>,
    mut seen: ResMut<Observed>,
    pointer: Option<Res<PointerRoute>>,
) {
    if !window.focused
        || !panel.open
        || panel.inspection.is_some()
        || !session.playing(player.0)
        || !screenshots.is_empty()
    {
        return;
    }
    let Some(grid) = inventory
        .owned_grid(session.raid.raid_id, player.0)
        .filter(known_grid)
    else {
        return;
    };
    if grid.items.is_empty() {
        return;
    }
    let viewport = Vec2::new(
        window.physical_width() as f32,
        window.physical_height() as f32,
    );
    let mut root = None;
    let mut board = None;
    let mut feedback = None;
    let mut tooltip = None;
    let mut context = None;
    let mut controls = Vec::new();
    let mut ghost = None;
    for (
        node,
        transform,
        (is_root, is_board, is_tooltip, is_context, is_action, is_feedback),
        kind,
    ) in &nodes
    {
        if node.size().min_element() <= 0.0 {
            continue;
        }
        let rect = layout_rect(node, transform);
        if is_root {
            root = Some(rect);
        }
        if is_board {
            board = Some(rect);
        }
        if is_feedback {
            feedback = Some(rect);
        }
        if is_tooltip {
            tooltip = Some(rect);
        }
        if is_context {
            context = Some(rect);
        }
        if is_action {
            controls.push(rect);
        }
        if kind.is_some() {
            ghost = kind.copied();
        }
    }
    let (Some(root), Some(board), Some(feedback)) = (root, board, feedback) else {
        return;
    };
    let state = if panel.context.is_some() {
        if context.is_none() || controls.len() != CONTEXT_ACTIONS.len() {
            return;
        }
        "context"
    } else {
        match (
            panel.pending.is_some(),
            panel.drag.is_some(),
            ghost,
            tooltip.is_some(),
        ) {
            (true, _, Some(GhostKind::Pending), _) => "pending",
            (false, true, Some(GhostKind::Valid), _) => "drag-valid",
            (false, true, Some(GhostKind::Invalid), _) => "drag-invalid",
            (false, false, None, true) => "hover",
            (false, false, None, false) => "idle",
            _ => return,
        }
    };
    if matches!(state, "hover" | "context")
        && match pointer.as_deref() {
            Some(route) => {
                route.target.is_none()
                    || (route.close_context
                        && (!seen.0.contains("idle") || (state == "context" && !route.pressed)))
            }
            None => output.automated,
        }
    {
        return;
    }
    if !seen.0.insert(state) {
        return;
    }
    let inside = [root, board, feedback]
        .into_iter()
        .chain(tooltip)
        .chain(context)
        .chain(controls)
        .all(|rect| fits(rect, viewport));
    // Log off-screen geometry as a finding, not a reason to suppress the
    // image. The invalid ghost itself may deliberately extend past the board.
    info!(
        "GRID REVIEW {state} {}x{} ui_scale={} revision={} items={} layout={} root={root:?} board={board:?} feedback={feedback:?} tooltip={tooltip:?} context={context:?}",
        viewport.x,
        viewport.y,
        scale.0,
        grid.version.revision,
        grid.items.len(),
        if inside { "inside" } else { "CLIPPED" }
    );
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(
            output.directory.join(format!("grid-{state}.png")),
        ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_grid_pointer_uses_physical_cell_coordinates_and_restores_window_state() {
        let size = [4, 4];
        let local = cell_center(
            size,
            Vec2::splat(200.0),
            Placement {
                x: 2,
                y: 1,
                rotation: Rotation::QuarterTurn,
            },
        );
        let transform = UiGlobalTransform::from_translation(Vec2::new(600.0, 300.0));
        let point = transform * local;
        assert_eq!(point, Vec2::new(625.0, 275.0));
        let mut app = App::new();
        let mut window = Window::default();
        window.set_physical_cursor_position(Some(Vec2::new(12.0, 24.0).as_dvec2()));
        let saved = window.internal;
        window.set_physical_cursor_position(Some(point.as_dvec2()));
        let entity = app.world_mut().spawn((PrimaryWindow, window)).id();
        app.insert_resource(PointerRoute {
            saved_window_state: Some(saved),
            ..default()
        })
        .add_systems(PostUpdate, restore_pointer);
        app.update();
        assert_eq!(app.world().get::<Window>(entity).unwrap().internal, saved);
        assert!(
            app.world()
                .resource::<PointerRoute>()
                .saved_window_state
                .is_none()
        );
    }

    #[test]
    fn inventory_grid_review_checks_physical_bounds_at_review_sizes() {
        let node = ComputedNode {
            size: Vec2::new(420.0, 490.0),
            ..default()
        };
        for viewport in [
            Vec2::new(1280.0, 720.0),
            Vec2::new(1280.0, 800.0),
            Vec2::new(1920.0, 1080.0),
        ] {
            let inside = UiGlobalTransform::from_translation(Vec2::new(viewport.x - 230.0, 313.0));
            let clipped = UiGlobalTransform::from_translation(Vec2::new(viewport.x - 190.0, 313.0));
            assert!(fits(layout_rect(&node, &inside), viewport));
            assert!(!fits(layout_rect(&node, &clipped), viewport));
        }
    }
}
