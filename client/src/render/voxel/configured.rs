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
                (WeaponId::M4A1, "m4a1_baseline".into()),
                (WeaponId::M1911, "m1911_baseline".into()),
                (WeaponId::MP5, "mp5_baseline".into()),
            ]
            .into(),
            components: [
                (DefinitionId(101), "m4a1_magazine_baseline".into()),
                (DefinitionId(102), "m1911_magazine_baseline".into()),
                (DefinitionId(103), "mp5_magazine_baseline".into()),
            ]
            .into(),
        }
    }
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConfiguredWeapon {
    pub epoch: u64,
    pub handle: u32,
    pub view_model: bool,
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
    pub preferred_root: Transform,
    pub shoulders: [Vec3; 2],
    pub poles: [Vec3; 2],
    pub stock_target: Option<Vec3>,
    pub max_adjustment: f32,
    pub support_blends: Vec<SocketBlend>,
    pub motions: Vec<PartMotion>,
    pub hidden_parts: Vec<(u16, String)>,
    pub visible_hands: [bool; 6],
}

/// Last atomically accepted solution, in the request entity's frame.
#[derive(Component, Clone, Copy)]
pub(crate) struct ConfiguredPose {
    pub solution: handling::HandlingSolution,
    pub shoulders: [Vec3; 2],
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
    hands: [Entity; 6], // support upper/lower/palm, then trigger
}

struct Piece {
    entity: Entity,
    definition: u32,
    asset: String,
    pose: Transform,
}

#[derive(Default, Resource)]
pub(super) struct HandAssets {
    handles: Option<(
        Handle<Mesh>,
        Handle<Mesh>,
        Handle<Mesh>,
        Handle<StandardMaterial>,
    )>,
}

impl HandAssets {
    fn load(
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
                meshes.add(super::super::weapons::glove()),
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
            let contacts = WeaponContacts::from_assembly(&entry.resolved.assembly)
                .map_err(|e| format!("weapon contacts: {e:?}"))?;
            let root_asset = bindings
                .weapons
                .get(&entry.packet.weapon.definition)
                .ok_or_else(|| "weapon visual binding is missing".to_owned())?;
            let mut pieces = vec![(
                0,
                entry.packet.weapon.definition.0,
                root_asset.clone(),
                Transform::IDENTITY,
            )];
            for part in &entry.resolved.assembly.parts {
                let asset = bindings.components.get(&part.definition).ok_or_else(|| {
                    format!("component {} visual binding is missing", part.definition.0)
                })?;
                pieces.push((part.node, part.definition.0, asset.clone(), part.pose));
            }
            for (_, _, asset, _) in &pieces {
                if !BAKED_ASSETS.iter().any(|(id, _)| id == asset) {
                    return Err(format!("unknown voxel asset '{asset}'"));
                }
            }
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
    )>,
    instances: Query<(&VoxelInstance, &ConfiguredPiece)>,
    mut transforms: Query<(&mut Transform, &mut Visibility)>,
) {
    for (entity, model, request, input, old_error) in &models {
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
                    updates.push((
                        model.hands[i * 3 + j],
                        segment.transform(),
                        input.visible_hands[i * 3 + j],
                    ));
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
                    });
            }
            Err(error) => report_error(&mut commands, entity, error, old_error),
        }
    }
}

#[cfg(test)]
mod tests;

pub(crate) mod native;
