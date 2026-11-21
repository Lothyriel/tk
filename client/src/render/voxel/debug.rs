//! Opt-in socket/pivot inspection of the live hierarchy, in each camera layer.
//! No alternate coordinates or preview meshes: markers follow accepted parts.
use super::*;

pub(crate) struct Plugin;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct WorldAxes;
#[derive(Default, Reflect, GizmoConfigGroup)]
struct ViewAxes;

#[derive(Resource)]
pub(crate) struct DebugView {
    enabled: bool,
    pub lod: Option<bool>,
}

#[derive(Component)]
struct Legend;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<WorldAxes>()
            .init_gizmo_group::<ViewAxes>()
            .insert_resource(DebugView {
                enabled: std::env::var_os("TK_WEAPON_DEBUG").is_some()
                    && std::env::var_os("TK_PLAYTEST_CLEAN_ART").is_none(),
                lod: if std::env::var_os("TK_WEAPON_DEBUG").is_some() {
                    match std::env::var("TK_WEAPON_LOD").as_deref() {
                        Ok("near") => Some(false),
                        Ok("far") => Some(true),
                        Ok("auto") | Err(_) => None,
                        Ok(value) => panic!("unknown TK_WEAPON_LOD {value}"),
                    }
                } else {
                    None
                },
            })
            .add_systems(Startup, setup)
            .add_systems(Update, toggle)
            .add_systems(
                PostUpdate,
                draw.after(bevy::transform::TransformSystems::Propagate)
                    .after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
            );
    }
}

fn setup(mut commands: Commands, mut configs: ResMut<GizmoConfigStore>) {
    for (config, layer) in [
        (configs.config_mut::<WorldAxes>().0.clone(), 0),
        (
            configs.config_mut::<ViewAxes>().0.clone(),
            VIEW_MODEL_RENDER_LAYER,
        ),
    ] {
        let mut config = config;
        config.render_layers = RenderLayers::layer(layer);
        config.depth_bias = -0.001;
        config.line.width = 2.0;
        if layer == 0 {
            configs.insert(config, WorldAxes);
        } else {
            configs.insert(config, ViewAxes);
        }
    }
    commands.spawn((
        Legend,
        Text::new(""),
        TextFont::from_font_size(14.0),
        TextColor(Color::WHITE),
        BackgroundColor(Color::srgba(0.01, 0.02, 0.03, 0.85)),
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            bottom: px(40),
            ..default()
        },
    ));
}

fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    mut debug: ResMut<DebugView>,
    mut legends: Query<(&mut Text, &mut Node), With<Legend>>,
) {
    if keys.just_pressed(KeyCode::F8) {
        debug.enabled = !debug.enabled;
        if !debug.enabled {
            debug.lod = None;
        }
    }
    if debug.enabled && keys.just_pressed(KeyCode::F9) {
        debug.lod = match debug.lod {
            None => Some(false),
            Some(false) => Some(true),
            Some(true) => None,
        };
    }
    for (mut text, mut node) in &mut legends {
        node.display = if debug.enabled {
            Display::Flex
        } else {
            Display::None
        };
        if debug.enabled {
            **text = format!(
                "WEAPON RIG | F8 overlay | F9 LOD: {}\n\
                 Sockets: X red / Y green / -Z blue (30 mm)\n\
                 Pivots: white cross (8 mm) | muzzle/bore amber\n\
                 sights cyan | grips green | ejection orange | controls magenta",
                match debug.lod {
                    None => "auto",
                    Some(false) => "near",
                    Some(true) => "far (inspection)",
                }
            );
        }
    }
}

fn draw(
    debug: Res<DebugView>,
    points: Query<
        (
            &Name,
            &GlobalTransform,
            &InheritedVisibility,
            &RenderLayers,
            Has<VoxelSocket>,
        ),
        Or<(With<VoxelSocket>, With<VoxelPart>)>,
    >,
    mut world: Gizmos<WorldAxes>,
    mut view: Gizmos<ViewAxes>,
) {
    if !debug.enabled {
        return;
    }
    for (name, pose, visible, layers, socket) in &points {
        if !visible.get() {
            continue;
        }
        let lines = marker_lines(name.as_str(), pose, socket);
        for (start, end, color) in lines {
            if layers.intersects(&RenderLayers::layer(VIEW_MODEL_RENDER_LAYER)) {
                view.line(start, end, color);
            } else {
                world.line(start, end, color);
            }
        }
    }
}

fn marker_lines(name: &str, pose: &GlobalTransform, socket: bool) -> Vec<(Vec3, Vec3, Color)> {
    let origin = pose.translation();
    let mut lines = Vec::with_capacity(6);
    if !socket {
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            lines.push((origin - axis * 0.004, origin + axis * 0.004, Color::WHITE));
        }
        return lines;
    }
    for (axis, color) in [
        (Vec3::X, Color::srgb(1.0, 0.1, 0.1)),
        (Vec3::Y, Color::srgb(0.1, 1.0, 0.1)),
        (Vec3::NEG_Z, Color::srgb(0.15, 0.45, 1.0)),
    ] {
        lines.push((origin, pose.transform_point(axis * 0.03), color));
    }
    let color = if name.starts_with("bore") || name.starts_with("muzzle") {
        Color::srgb(1.0, 0.8, 0.05)
    } else if name.starts_with("sight") {
        Color::srgb(0.0, 1.0, 1.0)
    } else if name.starts_with("control") {
        Color::srgb(1.0, 0.1, 1.0)
    } else if name == "ejection" {
        Color::srgb(1.0, 0.35, 0.0)
    } else {
        Color::srgb(0.4, 1.0, 0.4)
    };
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        lines.push((origin - axis * 0.003, origin + axis * 0.003, color));
    }
    lines
}
