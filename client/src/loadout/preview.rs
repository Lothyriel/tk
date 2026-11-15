use super::*;
use crate::render::voxel::configured::{ConfiguredHandling, ConfiguredWeapon};
use bevy::{camera::RenderTarget, render::render_resource::TextureFormat};
use common::humanoid::handling::WeaponContacts;

#[derive(Resource)]
pub(super) struct PreviewImage(pub Handle<Image>);
#[derive(Component)]
pub(super) struct PreviewCamera;
#[derive(Component)]
pub(super) struct PreviewWeapon;
const ORIGIN: Vec3 = Vec3::new(10_000.0, 10_000.0, 10_000.0);

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

pub(super) fn update(
    mut commands: Commands,
    editor: Res<Editor>,
    session: Res<ClientSession>,
    id: Res<PlayerId>,
    mut camera: Single<(&mut Camera, &mut Transform), With<PreviewCamera>>,
    mut model: Single<(Entity, &mut Visibility, Option<&ConfiguredWeapon>), With<PreviewWeapon>>,
) {
    camera.0.is_active = editor.visible;
    *model.1 = if editor.visible {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if !editor.visible {
        return;
    }
    let Some(entry) = session.configurations.cache.latest(id.0, editor.slot) else {
        return;
    };
    let request = ConfiguredWeapon {
        epoch: entry.packet.epoch,
        handle: entry.packet.handle,
        view_model: false,
        magazine: None,
    };
    if model.2 == Some(&request) {
        return;
    }
    let Ok(contacts) = WeaponContacts::from_assembly(&entry.resolved.assembly) else {
        return;
    };
    let handling = ConfiguredHandling {
        preflight_error: None,
        preferred_root: Transform::IDENTITY,
        shoulders: [
            contacts.support.translation + Vec3::Y * 0.3,
            contacts.trigger.translation + Vec3::Y * 0.3,
        ],
        poles: [Vec3::Z, -Vec3::Z],
        stock_target: None,
        max_adjustment: 0.0,
        support_blends: Vec::new(),
        motions: Vec::new(),
        hidden_parts: Vec::new(),
        visible_hands: [false; 6],
    };
    let mut min = Vec3::splat(-0.05);
    let mut max = Vec3::splat(0.05);
    for bounds in &entry.resolved.assembly.obstruction {
        min = min.min(Vec3::from(bounds.min));
        max = max.max(Vec3::from(bounds.max));
    }
    for anchor in entry.resolved.assembly.anchors.values() {
        min = min.min(anchor.translation);
        max = max.max(anchor.translation);
    }
    let center = ORIGIN + (min + max) * 0.5;
    let distance = (max - min).length().max(0.4) * 1.4;
    *camera.1 =
        Transform::from_translation(center + Vec3::new(1.0, 0.3, 0.12).normalize() * distance)
            .looking_at(center, Vec3::Y);
    commands.entity(model.0).insert((request, handling));
}
