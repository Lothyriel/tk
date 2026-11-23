//! Configuration revision -> shared baked piece instances -> rigid contacts.
use super::*;
use crate::sync::ClientSession;
use common::{
    catalog::{DefinitionId, WeaponId},
    humanoid::handling::{self, WeaponContacts},
};

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PoseConfigured;

/// Visual bindings belong to the client. Catalogue/network data never supplies
/// filesystem paths or mesh handles. Node zero is the weapon definition.
#[derive(Resource)]
pub(crate) struct VisualBindings {
    pub weapons: BTreeMap<WeaponId, String>,
    pub components: BTreeMap<DefinitionId, String>,
}

impl Default for VisualBindings {
    fn default() -> Self {
        Self {
            weapons: [
                (WeaponId::M4A1, "m4a1_receiver".into()),
                (WeaponId::M1911, "m1911_baseline".into()),
                (WeaponId::MP5, "mp5_baseline".into()),
            ]
            .into(),
            components: [
                (DefinitionId(101), "m4a1_magazine_30".into()),
                (DefinitionId(102), "m1911_magazine_baseline".into()),
                (DefinitionId(103), "mp5_magazine_baseline".into()),
                (DefinitionId(120), "m4a1_pmag_20".into()),
                (DefinitionId(121), "m4a1_stock".into()),
                (DefinitionId(122), "m4a1_ctr".into()),
                (DefinitionId(123), "m4a1_ras".into()),
                (DefinitionId(124), "m4a1_moe_sl".into()),
                (DefinitionId(125), "m4a1_barrel_145".into()),
                (DefinitionId(126), "m4a1_dd_103".into()),
                (DefinitionId(127), "m4a1_a2".into()),
                (DefinitionId(128), "m4a1_sfmb".into()),
                (DefinitionId(129), "m4a1_rc2".into()),
                (DefinitionId(130), "m4a1_front_iron".into()),
                (DefinitionId(131), "m4a1_rear_iron".into()),
                (DefinitionId(132), "m4a1_qrp2".into()),
                (DefinitionId(133), "m4a1_pro".into()),
                (DefinitionId(134), "m4a1_rvg".into()),
                (DefinitionId(135), "m4a1_afg2".into()),
            ]
            .into(),
        }
    }
}

pub(crate) type BoundPiece = (u16, u32, String, Transform);

impl VisualBindings {
    pub(crate) fn pieces(
        &self,
        spec: &common::catalog::ResolvedWeapon,
    ) -> Result<Vec<BoundPiece>, String> {
        let asset = self
            .weapons
            .get(&spec.definition)
            .ok_or_else(|| "weapon visual binding is missing".to_owned())?;
        let mut pieces = vec![(0, spec.definition.0, asset.clone(), Transform::IDENTITY)];
        for part in &spec.assembly.parts {
            let asset = self.components.get(&part.definition).ok_or_else(|| {
                format!("component {} visual binding is missing", part.definition.0)
            })?;
            pieces.push((part.node, part.definition.0, asset.clone(), part.pose));
        }
        for (_, _, asset, _) in &pieces {
            if !BAKED_ASSETS.iter().any(|(id, _)| id == asset) {
                return Err(format!("unknown voxel asset '{asset}'"));
            }
        }
        Ok(pieces)
    }
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConfiguredWeapon {
    pub epoch: u64,
    pub handle: u32,
    pub view_model: bool,
    /// A physical magazine from this configuration's reliable pool. Only its
    /// visual/contact variant is selected; installed gameplay stats stay intact.
    pub magazine: Option<common::catalog::MagazineInstanceId>,
}

#[derive(Clone)]
pub(crate) struct SocketAddress {
    pub node: u16,
    pub socket: String,
}

#[derive(Clone)]
pub(crate) struct SocketBlend {
    pub address: SocketAddress,
    pub weight: f32,
}

#[derive(Clone)]
pub(crate) struct PartMotion {
    pub node: u16,
    pub part: String,
    /// Delta after the authored pivot, in that part's frame.
    pub delta: Transform,
}

/// All coordinates are relative to the entity carrying ConfiguredWeapon.
/// Its parent supplies actor/camera frame conversion. Controls supply the
/// preferred root and mechanism deltas; this system owns the accepted pose.
#[derive(Component, Clone)]
pub(crate) struct ConfiguredHandling {
    /// A producer rejection must preserve the accepted pose, not fall back to
    /// an unbraced request that silently drops a required contact.
    pub preflight_error: Option<String>,
    pub preferred_root: Transform,
    pub shoulders: [Vec3; 2],
    pub poles: [Vec3; 2],
    pub stock_target: Option<Vec3>,
    pub max_adjustment: f32,
    pub support_blends: Vec<SocketBlend>,
    pub support_clearance: Vec3,
    pub support_fingers: [f32; 2],
    pub motions: Vec<PartMotion>,
    pub hidden_parts: Vec<(u16, String)>,
    pub visible_hands: [bool; 6],
}

/// Last atomically accepted solution, in the request entity's frame.
#[derive(Component, Clone, Copy)]
pub(crate) struct ConfiguredPose {
    pub solution: handling::HandlingSolution,
    pub shoulders: [Vec3; 2],
    /// Device outlet in request space, from the last accepted assembly.
    pub muzzle: Transform,
}

/// View-only sleeve geometry. The solved bones and grip sockets stay intact.
#[derive(Component)]
pub(crate) struct ViewSleeveClearance {
    pub to_eye: Mat4,
    pub near: f32,
}

impl ViewSleeveClearance {
    fn trim(&self, pose: &mut Transform, length: f32, half_width: f32, half_depth: f32) -> bool {
        let z = |point| self.to_eye.transform_point3(point).z;
        let radial = self
            .to_eye
            .transform_vector3(pose.rotation * Vec3::X)
            .z
            .abs()
            * half_width
            + self
                .to_eye
                .transform_vector3(pose.rotation * Vec3::Z)
                .z
                .abs()
                * half_depth;
        // Keep sleeves out of the lens's immediate foreground as well as
        // ahead of its near plane. Their palm contacts are never shortened.
        let limit = -(self.near + 0.01).max(0.16) - radial;
        let axis = pose.rotation * Vec3::Y;
        let a = pose.translation - axis * (length * 0.5);
        let b = pose.translation + axis * (length * 0.5);
        let (za, zb) = (z(a), z(b));
        if za.max(zb) <= limit {
            return true;
        }
        if za.min(zb) >= limit {
            return false;
        }
        let cut = a.lerp(b, (limit - za) / (zb - za));
        let kept = if za < zb { a } else { b };
        pose.translation = kept.midpoint(cut);
        pose.scale.y *= kept.distance(cut) / length;
        true
    }
}

#[derive(Component)]
pub(crate) struct ConfigurationVisualError(pub String);

#[derive(Component)]
pub(crate) struct ConfiguredPiece {
    pub node: u16,
    pub definition: u32,
}

#[derive(Component)]
pub(super) struct Instance {
    request: ConfiguredWeapon,
    weapon_instance: common::WeaponInstanceId,
    revision: u64,
    content: Entity,
    weapon_root: Entity,
    pieces: BTreeMap<u16, Piece>,
    contacts: WeaponContacts,
    muzzle: Transform,
    hands: [Entity; 6], // support upper/lower/palm, then trigger
}

struct Piece {
    entity: Entity,
    definition: u32,
    asset: String,
    pose: Transform,
}

#[derive(Default, Resource)]
pub(in crate::render) struct HandAssets {
    handles: Option<(
        Handle<Mesh>,
        Handle<Mesh>,
        Handle<Mesh>,
        Handle<StandardMaterial>,
    )>,
}

impl HandAssets {
    pub(in crate::render) fn load(
        &mut self,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) -> &(
        Handle<Mesh>,
        Handle<Mesh>,
        Handle<Mesh>,
        Handle<StandardMaterial>,
    ) {
        self.handles.get_or_insert_with(|| {
            (
                meshes.add(Cuboid::new(0.07, handling::UPPER_ARM_METRES, 0.065)),
                meshes.add(Cuboid::new(0.065, handling::FOREARM_METRES, 0.06)),
                meshes.add(super::super::weapons::grasp::mesh()),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.11, 0.14, 0.10),
                    ..default()
                }),
            )
        })
    }
}

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct AssemblyAssets<'w> {
    cache: ResMut<'w, VoxelCache>,
    hands: ResMut<'w, HandAssets>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
}

fn report_error(
    commands: &mut Commands,
    entity: Entity,
    error: String,
    old: Option<&ConfigurationVisualError>,
) {
    if old.is_none_or(|old| old.0 != error) {
        warn!("Configured weapon {entity:?}: {error}");
        commands
            .entity(entity)
            .insert(ConfigurationVisualError(error));
    }
}

pub(super) fn assemble(
    mut commands: Commands,
    session: Option<Res<ClientSession>>,
    bindings: Res<VisualBindings>,
    assets: AssemblyAssets,
    models: Query<(
        Entity,
        &ConfiguredWeapon,
        Option<&Instance>,
        Option<&ConfiguredHandling>,
        Option<&ConfigurationVisualError>,
    )>,
    removed: Query<
        (Entity, Option<&Instance>),
        (
            Without<ConfiguredWeapon>,
            Or<(With<Instance>, With<ConfigurationVisualError>)>,
        ),
    >,
) {
    let AssemblyAssets {
        mut cache,
        mut hands,
        mut meshes,
        mut materials,
    } = assets;
    for (entity, instance) in &removed {
        if let Some(instance) = instance {
            commands.entity(instance.content).despawn();
        }
        commands
            .entity(entity)
            .remove::<(Instance, ConfigurationVisualError, ConfiguredPose)>();
    }
    let Some(session) = session else {
        return;
    };
    for (entity, request, previous, input, old_error) in &models {
        if previous.is_some_and(|p| p.request == *request) {
            continue;
        }
        let Some(input) = input else {
            report_error(
                &mut commands,
                entity,
                "handling input is missing".into(),
                old_error,
            );
            continue;
        };
        let prepare = || -> Result<_, String> {
            let entry = session
                .configurations
                .cache
                .get(request.epoch, request.handle)
                .ok_or_else(|| "configuration is not available".to_owned())?;
            if let Some(old) = previous
                && old.weapon_instance == entry.packet.instance
                && request.epoch <= old.request.epoch
                && entry.packet.revision < old.revision
            {
                return Err("stale configuration revision".into());
            }
            let visual = entry
                .visual(request.magazine)
                .ok_or("magazine is not in the owned pool")?;
            let contacts = WeaponContacts::from_assembly(&visual.assembly)
                .map_err(|e| format!("weapon contacts: {e:?}"))?;
            let pieces = bindings.pieces(visual)?;
            Ok((entry, contacts, pieces))
        };
        let (entry, contacts, pieces) = match prepare() {
            Ok(prepared) => prepared,
            Err(error) => {
                report_error(&mut commands, entity, error, old_error);
                continue;
            }
        };
        // Preload every validated binding before publishing a replacement.
        let mut failure = None;
        for (_, _, asset, _) in &pieces {
            if let Err(error) = cache.load(asset, &mut meshes, &mut materials) {
                failure = Some(error);
                break;
            }
        }
        if let Some(error) = failure {
            report_error(&mut commands, entity, error, old_error);
            continue;
        }
        if let Err(error) = resolve_pose(
            &cache,
            contacts,
            input,
            pieces
                .iter()
                .map(|(node, _, asset, pose)| (*node, asset.as_str(), *pose)),
        ) {
            report_error(&mut commands, entity, error, old_error);
            continue;
        }
        let content = commands
            .spawn((
                Name::new("configured-weapon"),
                Transform::default(),
                Visibility::Inherited,
            ))
            .id();
        let weapon_root = commands
            .spawn((Transform::default(), Visibility::Inherited))
            .id();
        commands.entity(content).add_child(weapon_root);
        let pieces = pieces
            .into_iter()
            .map(|(node, definition, asset, pose)| {
                let child = commands
                    .spawn((
                        ConfiguredPiece { node, definition },
                        VoxelModel {
                            asset: asset.clone(),
                            view_model: request.view_model,
                        },
                        pose,
                        Visibility::Inherited,
                    ))
                    .id();
                // Resolver poses already compose the attachment tree into root space.
                commands.entity(weapon_root).add_child(child);
                (
                    node,
                    Piece {
                        entity: child,
                        definition,
                        asset,
                        pose,
                    },
                )
            })
            .collect();
        let handles = hands.load(&mut meshes, &mut materials);
        let hands = std::array::from_fn(|i| {
            let mesh = match i % 3 {
                0 => &handles.0,
                1 => &handles.1,
                _ => &handles.2,
            };
            let mut hand = commands.spawn((
                Name::new(format!(
                    "configured-{}-{}",
                    if i < 3 { "support" } else { "trigger" },
                    ["upper", "forearm", "palm"][i % 3]
                )),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(handles.3.clone()),
                Transform::default(),
                Visibility::Hidden,
                RenderLayers::layer(if request.view_model {
                    VIEW_MODEL_RENDER_LAYER
                } else {
                    0
                }),
            ));
            if i % 3 == 2 {
                hand.insert((
                    super::super::weapons::grasp::component([0.0; 2]),
                    super::super::weapons::grasp::bounds(),
                ));
            }
            if request.view_model {
                hand.insert(NotShadowCaster);
            }
            let child = hand.id();
            commands.entity(content).add_child(child);
            child
        });
        commands.entity(entity).add_child(content);
        if let Some(old) = previous {
            commands.entity(old.content).despawn();
        }
        commands
            .entity(entity)
            .remove::<ConfigurationVisualError>()
            .insert(Instance {
                request: *request,
                weapon_instance: entry.packet.instance,
                revision: entry.packet.revision,
                content,
                weapon_root,
                pieces,
                contacts,
                muzzle: common::weapon_animation::configured_outlet(&entry.resolved),
                hands,
            });
    }
}

fn rigid(pose: Transform) -> bool {
    pose.translation.is_finite()
        && pose.rotation.is_finite()
        && pose.scale.is_finite()
        && (pose.rotation.length_squared() - 1.0).abs() < 1e-5
        && pose.scale.distance(Vec3::ONE) < 1e-6
}

/// Calculate an asset's moving hierarchy without touching ECS. Returns authored
/// local pivots and fully composed root-local socket poses for an atomic commit.
fn part_poses(
    asset: &CachedAsset,
    node: u16,
    motions: &[PartMotion],
) -> Result<BTreeMap<String, (Transform, Transform)>, String> {
    let mut local: BTreeMap<_, _> = asset
        .parts
        .iter()
        .map(|p| (p.id.clone(), p.pivot))
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    for motion in motions.iter().filter(|m| m.node == node) {
        let part = asset
            .parts
            .iter()
            .find(|p| p.id == motion.part)
            .ok_or_else(|| format!("unknown moving part {}", motion.part))?;
        if !part.movable || !rigid(motion.delta) || !seen.insert(&motion.part) {
            return Err(format!("invalid motion for part {}", motion.part));
        }
        local.insert(motion.part.clone(), part.pivot.mul_transform(motion.delta));
    }
    let mut result = BTreeMap::new();
    for part in &asset.parts {
        let mut composed = local[&part.id];
        let mut parent = part.parent.as_deref();
        while let Some(id) = parent {
            composed = local[id].mul_transform(composed);
            parent = asset
                .parts
                .iter()
                .find(|p| p.id == id)
                .unwrap()
                .parent
                .as_deref();
        }
        result.insert(part.id.clone(), (local[&part.id], composed));
    }
    Ok(result)
}

fn resolve_pose<'a>(
    cache: &VoxelCache,
    mut contacts: WeaponContacts,
    input: &ConfiguredHandling,
    pieces: impl Iterator<Item = (u16, &'a str, Transform)>,
) -> Result<(handling::HandlingSolution, Vec<(u16, String, Transform)>), String> {
    if let Some(error) = &input.preflight_error {
        return Err(error.clone());
    }
    let mut updates = Vec::new();
    let mut nodes = std::collections::BTreeSet::new();
    let mut sockets = BTreeMap::new();
    for (node, asset, piece_pose) in pieces {
        nodes.insert(node);
        let asset = &cache.assets[asset];
        let poses = part_poses(asset, node, &input.motions)?;
        for (name, (local, _)) in &poses {
            updates.push((node, name.clone(), *local));
        }
        for blend in &input.support_blends {
            let address = &blend.address;
            if address.node != node {
                continue;
            }
            let socket = asset
                .sockets
                .iter()
                .find(|s| s.id == address.socket)
                .ok_or_else(|| format!("unknown socket {}", address.socket))?;
            sockets.insert(
                (node, address.socket.as_str()),
                piece_pose
                    .mul_transform(poses[&socket.part].1)
                    .mul_transform(socket.pose.transform()),
            );
        }
        if input
            .hidden_parts
            .iter()
            .any(|(n, part)| *n == node && !poses.contains_key(part))
        {
            return Err("visibility references unknown part".into());
        }
    }
    if input.motions.iter().any(|m| !nodes.contains(&m.node)) {
        return Err("motion references unknown configuration node".into());
    }
    if input
        .hidden_parts
        .iter()
        .any(|(node, _)| !nodes.contains(node))
    {
        return Err("visibility references unknown configuration node".into());
    }
    for blend in &input.support_blends {
        if !blend.weight.is_finite() || !(0.0..=1.0).contains(&blend.weight) {
            return Err("invalid contact blend".into());
        }
        let target = sockets
            .get(&(blend.address.node, blend.address.socket.as_str()))
            .ok_or("socket references unknown configuration node")?;
        contacts = contacts
            .with_support(
                Transform::from_translation(
                    contacts
                        .support
                        .translation
                        .lerp(target.translation, blend.weight),
                )
                .with_rotation(
                    contacts
                        .support
                        .rotation
                        .slerp(target.rotation, blend.weight),
                ),
            )
            .map_err(|e| format!("socket contact: {e:?}"))?;
    }
    let mut support = contacts.support;
    support.translation += input.support_clearance;
    contacts = contacts
        .with_support(support)
        .map_err(|e| format!("support clearance: {e:?}"))?;
    let solved = handling::fit_weapon(
        input.preferred_root,
        contacts,
        input.shoulders,
        input.poles,
        input.stock_target,
        input.max_adjustment,
    )
    .map_err(|e| format!("weapon handling: {e:?}"))?;
    Ok((solved, updates))
}

pub(super) fn pose(
    mut commands: Commands,
    cache: Res<VoxelCache>,
    models: Query<(
        Entity,
        &Instance,
        &ConfiguredWeapon,
        &ConfiguredHandling,
        Option<&ConfigurationVisualError>,
        Option<&ViewSleeveClearance>,
    )>,
    instances: Query<(&VoxelInstance, &ConfiguredPiece)>,
    mut transforms: Query<(&mut Transform, &mut Visibility)>,
) {
    for (entity, model, request, input, old_error, sleeve) in &models {
        if *request != model.request {
            continue;
        }
        let prepare = || -> Result<_, String> {
            let (solved, part_updates) = resolve_pose(
                &cache,
                model.contacts,
                input,
                model
                    .pieces
                    .iter()
                    .map(|(&node, p)| (node, p.asset.as_str(), p.pose)),
            )?;
            let mut updates = Vec::new();
            for (node, name, local) in part_updates {
                let piece = &model.pieces[&node];
                let (instance, identity) = instances
                    .get(piece.entity)
                    .map_err(|_| "piece is not instantiated")?;
                if identity.node != node || identity.definition != piece.definition {
                    return Err(format!(
                        "definition {} has inconsistent node identity",
                        identity.definition
                    ));
                }
                let target = instance
                    .parts
                    .get(&name)
                    .ok_or("piece part is not instantiated")?;
                updates.push((*target, local, !input.hidden_parts.contains(&(node, name))));
            }
            updates.push((model.weapon_root, solved.root, true));
            for (i, arm) in [solved.support, solved.trigger].into_iter().enumerate() {
                for (j, segment) in arm.segments(input.shoulders[i]).into_iter().enumerate() {
                    let mut transform = segment.transform();
                    let mut visible = input.visible_hands[i * 3 + j];
                    if let Some(clearance) = sleeve {
                        visible &= match j {
                            0 => clearance.trim(
                                &mut transform,
                                handling::UPPER_ARM_METRES,
                                0.035,
                                0.0325,
                            ),
                            1 => clearance.trim(
                                &mut transform,
                                handling::FOREARM_METRES,
                                0.0325,
                                0.030,
                            ),
                            _ => true,
                        };
                    }
                    updates.push((model.hands[i * 3 + j], transform, visible));
                }
            }
            if updates
                .iter()
                .any(|(entity, _, _)| !transforms.contains(*entity))
            {
                return Err("pose target is missing".into());
            }
            Ok((solved, updates))
        };
        match prepare() {
            Ok((solved, updates)) => {
                commands
                    .entity(model.hands[2])
                    .insert(super::super::weapons::grasp::component(
                        input.support_fingers,
                    ));
                for (target, pose, visible) in updates {
                    if let Ok((mut transform, mut visibility)) = transforms.get_mut(target) {
                        *transform = pose;
                        *visibility = if visible {
                            Visibility::Inherited
                        } else {
                            Visibility::Hidden
                        };
                    }
                }
                commands
                    .entity(entity)
                    .remove::<ConfigurationVisualError>()
                    .insert(ConfiguredPose {
                        solution: solved,
                        shoulders: input.shoulders,
                        muzzle: solved.root.mul_transform(model.muzzle),
                    });
            }
            Err(error) => report_error(&mut commands, entity, error, old_error),
        }
    }
}

#[cfg(test)]
mod tests;

pub(crate) mod native;
