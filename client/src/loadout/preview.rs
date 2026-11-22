use super::*;
use crate::render::voxel::{
    VoxelModel,
    configured::{BoundPiece, VisualBindings},
};
use bevy::{camera::RenderTarget, render::render_resource::TextureFormat};
use common::catalog::{ResolvedWeapon, WeaponId, WeaponKit, catalog};

#[derive(Resource)]
pub(super) struct PreviewImage(pub Handle<Image>);
#[derive(Component)]
pub(crate) struct PreviewCamera;
#[derive(Component)]
pub(super) struct PreviewWeapon;
const ORIGIN: Vec3 = Vec3::new(10_000.0, 10_000.0, 10_000.0);

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum View {
    Confirmed,
    Draft,
    Pending,
    Invalid(String),
    Locked,
}

#[derive(Resource, Default)]
pub(crate) struct Preview {
    pub view: Option<View>,
    draft: Option<(WeaponKit, Result<ResolvedWeapon, String>)>,
    sockets: Vec<Option<Vec2>>,
}

impl Preview {
    pub(super) fn select<'a>(
        &'a mut self,
        editor: &'a Editor,
        confirmed: &'a ResolvedWeapon,
        accepted_kit: &'a WeaponKit,
        editable: bool,
    ) -> (&'a ResolvedWeapon, &'a WeaponKit) {
        let Some(kit) = editor.draft.as_ref().filter(|kit| *kit != accepted_kit) else {
            self.view = Some(if editor.pending.is_some() {
                View::Pending
            } else if editable {
                View::Confirmed
            } else {
                View::Locked
            });
            return (confirmed, accepted_kit);
        };
        if !editable && editor.pending.is_none() {
            self.view = Some(View::Locked);
            return (confirmed, accepted_kit);
        }
        if self.draft.as_ref().is_none_or(|(old, _)| old != kit) {
            self.draft = Some((
                kit.clone(),
                kit.resolve(catalog()).map_err(|e| e.to_string()),
            ));
        }
        match &self.draft.as_ref().unwrap().1 {
            Ok(spec) => {
                self.view = Some(if editor.pending.is_some() {
                    View::Pending
                } else {
                    View::Draft
                });
                (spec, kit)
            }
            Err(error) => {
                self.view = Some(View::Invalid(error.clone()));
                (confirmed, accepted_kit)
            }
        }
    }

    pub(super) fn socket(&self, row: usize) -> Option<Vec2> {
        self.sockets.get(row).copied().flatten()
    }
}

pub(super) fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let image = images.add(Image::new_target_texture(
        800,
        500,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    ));
    commands.spawn((
        PreviewCamera,
        Camera3d::default(),
        Camera {
            order: -1,
            is_active: false,
            clear_color: Color::srgb(0.10, 0.14, 0.18).into(),
            ..default()
        },
        AmbientLight {
            brightness: 2500.0,
            ..default()
        },
        RenderTarget::Image(image.clone().into()),
        Projection::Perspective(PerspectiveProjection {
            fov: 40.0_f32.to_radians(),
            aspect_ratio: 1.6,
            near: 0.01,
            far: 10.0,
            ..default()
        }),
        Transform::from_translation(ORIGIN + Vec3::X * 2.0).looking_at(ORIGIN, Vec3::Y),
    ));
    // Isolated preview space reuses the ordinary assembly pipeline and cache.
    // It is outside gameplay camera ranges, without a second mesh or resolver.
    commands.spawn((
        PreviewWeapon,
        Transform::from_translation(ORIGIN),
        Visibility::Hidden,
    ));
    commands.spawn((
        PointLight {
            intensity: 400_000.0,
            range: 8.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(ORIGIN + Vec3::new(2.0, 3.0, 1.0)),
    ));
    commands.insert_resource(PreviewImage(image));
}

#[derive(Default)]
pub(super) struct ModelState {
    framing: Option<WeaponId>,
    content: Option<(Entity, Vec<BoundPiece>)>,
    accepted: Option<((u64, u32), WeaponKit)>,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn update(
    mut commands: Commands,
    (editor, session, id): (Res<Editor>, Res<ClientSession>, Res<PlayerId>),
    bindings: Res<VisualBindings>,
    menu: Option<Res<crate::menu::Menu>>,
    mut camera: Single<(&mut Camera, &mut Transform, &Projection), With<PreviewCamera>>,
    mut model: Single<(Entity, &mut Visibility), With<PreviewWeapon>>,
    mut state: Local<ModelState>,
    mut preview: ResMut<Preview>,
) {
    camera.0.is_active = editor.visible;
    *model.1 = Visibility::Hidden;
    preview.view = None;
    preview.sockets.clear();
    if !editor.visible {
        return;
    }
    let Some(entry) = session.configurations.cache.latest(id.0, editor.slot) else {
        return;
    };
    let key = (entry.packet.epoch, entry.packet.handle);
    if state.accepted.as_ref().is_none_or(|(old, _)| *old != key) {
        state.accepted = entry.packet.kit(catalog()).ok().map(|kit| (key, kit));
    }
    let Some((_, kit)) = state.accepted.as_ref() else {
        return;
    };
    // The preview never registers a fake publication. It uses the same
    // visual bindings and baked mesh cache, but owns only its own entities.
    let (spec, kit) = preview.select(
        &editor,
        &entry.resolved,
        kit,
        editor.editable(&session, id.0) && !menu.is_some_and(|menu| menu.awaiting_lobby()),
    );
    let Ok(pieces) = bindings.pieces(spec) else {
        preview.view = None;
        return;
    };
    let positions = socket_positions(
        spec,
        kit,
        camera.2,
        if state.framing != Some(spec.definition) {
            *camera.1 = frame(&entry.resolved);
            *camera.1
        } else {
            *camera.1
        },
    );
    state.framing = Some(spec.definition);
    preview.sockets = positions;
    *model.1 = Visibility::Inherited;
    if state
        .content
        .as_ref()
        .is_some_and(|(_, old)| old == &pieces)
    {
        return;
    }
    if let Some((old, _)) = state.content.take() {
        commands.entity(old).despawn();
    }
    let content = commands
        .spawn((Transform::IDENTITY, Visibility::Inherited, ChildOf(model.0)))
        .with_children(|parent| {
            for (_, _, asset, pose) in &pieces {
                parent.spawn((
                    VoxelModel {
                        asset: asset.clone(),
                        view_model: false,
                    },
                    *pose,
                    Visibility::Inherited,
                ));
            }
        })
        .id();
    state.content = Some((content, pieces));
}

fn frame(spec: &ResolvedWeapon) -> Transform {
    let mut min = Vec3::splat(-0.05);
    let mut max = Vec3::splat(0.05);
    for bounds in &spec.assembly.obstruction {
        min = min.min(Vec3::from(bounds.min));
        max = max.max(Vec3::from(bounds.max));
    }
    for anchor in spec.assembly.anchors.values() {
        min = min.min(anchor.translation);
        max = max.max(anchor.translation);
    }
    let center = ORIGIN + (min + max) * 0.5;
    let distance = (max - min).length().max(0.4) * 1.55;
    Transform::from_translation(center + Vec3::new(1.0, 0.3, 0.12).normalize() * distance)
        .looking_at(center, Vec3::Y)
}

pub(super) fn socket_point(spec: &ResolvedWeapon, kit: &WeaponKit, index: usize) -> Option<Vec3> {
    let row = &editor::ROWS[index];
    if let Some(part) = spec.assembly.parts.iter().find(|p| p.node == row.node) {
        // Occupied sockets use the resolver's composed pose, including rail index.
        return Some(part.pose.translation);
    }
    if spec.definition != WeaponId::M4A1 {
        return None;
    }
    let (parent, geometry) = if let Some(parent) = row.parent {
        let part = spec
            .assembly
            .parts
            .iter()
            .find(|part| part.node == parent)?;
        (
            part.pose,
            &catalog().component(part.definition)?.assembly.geometry,
        )
    } else {
        (
            Transform::IDENTITY,
            &catalog()
                .definition(kit.weapon.definition)?
                .assembly
                .geometry,
        )
    };
    let socket = geometry
        .sockets
        .iter()
        .find(|socket| socket.key == row.socket)?;
    let pose = parent * socket.pose.transform();
    // Only position an empty marker; legality still belongs to kit.resolve.
    Some(pose.transform_point(Vec3::Z * socket.pitch_m * f32::from(editor::initial_position(row))))
}

fn socket_positions(
    spec: &ResolvedWeapon,
    kit: &WeaponKit,
    projection: &Projection,
    camera: Transform,
) -> Vec<Option<Vec2>> {
    let relative = camera.with_translation(camera.translation - ORIGIN);
    let clip = projection.get_clip_from_view() * relative.to_matrix().inverse();
    (0..editor::ROWS.len())
        .map(|row| {
            let ndc = clip.project_point3(socket_point(spec, kit, row)?);
            let xy = Vec2::new(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
            (xy.is_finite() && xy.min_element() >= 0.0 && xy.max_element() <= 1.0).then_some(xy)
        })
        .collect()
}
