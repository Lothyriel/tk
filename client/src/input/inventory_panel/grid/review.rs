//! Read the live grid after layout; never stage input, inventory, or pending.
use std::{collections::HashSet, path::PathBuf};

use bevy::{
    render::view::screenshot::{Capturing, Screenshot, save_to_disk},
    ui::{UiGlobalTransform, UiSystems},
};

use super::*;

pub(crate) struct GridReview {
    pub automated: bool,
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
        app.insert_resource(Output {
            directory,
            automated: self.automated,
        })
        .add_systems(PostUpdate, observe.after(UiSystems::PostLayout));
    }
}

#[derive(Resource)]
struct Output {
    directory: PathBuf,
    automated: bool,
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
    mut seen: Local<HashSet<&'static str>>,
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
    if (output.automated && matches!(state, "hover" | "context")) || !seen.insert(state) {
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
