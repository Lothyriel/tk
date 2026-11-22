//! Offline-baked asset surfaces shared across actors and camera perspectives.
//! Assembly is per rigid part/material, never per voxel or per frame.
use std::collections::BTreeMap;

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::RenderLayers,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use serde::Deserialize;

use super::VIEW_MODEL_RENDER_LAYER;

include!(concat!(env!("OUT_DIR"), "/voxel_registry.rs"));

pub(crate) mod item_review;

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VoxelCache>()
            .init_resource::<configured::VisualBindings>()
            .init_resource::<configured::HandAssets>()
            .add_systems(
                Update,
                (
                    configured::assemble,
                    remove_models,
                    instantiate,
                    configured::pose.in_set(configured::PoseConfigured),
                )
                    .chain()
                    .after(super::actors::AnimateActors)
                    .after(super::feedback::PrepareLocal)
                    .in_set(crate::ClientSystems::Present),
            )
            .add_systems(
                PostUpdate,
                lod::select
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(bevy::camera::visibility::VisibilitySystems::CalculateBounds),
            );
    }
}

/// Insert on an existing root with Transform/Visibility. A change replaces the
/// owned hierarchy atomically after the new asset has loaded successfully.
#[derive(Component, Clone, PartialEq, Eq)]
pub(crate) struct VoxelModel {
    pub asset: String,
    pub view_model: bool,
}

#[derive(Component)]
struct VoxelInstance {
    source: VoxelModel,
    content: Entity,
    surfaces: Vec<Entity>,
    far: bool,
    parts: BTreeMap<String, Entity>,
}

/// Named part entities are the animation integration boundary. Their Transform
/// is the authored pivot; child surfaces and sockets follow it automatically.
#[derive(Component)]
pub(crate) struct VoxelPart;

#[derive(Component)]
pub(crate) struct MovableVoxelPart;

#[derive(Component)]
pub(crate) struct VoxelSocket;

#[derive(Component)]
pub(crate) struct VoxelLoadError(pub String);

#[derive(Deserialize, Clone)]
struct Pose {
    translation: [f32; 3],
    rotation: [f32; 4],
}

impl Pose {
    fn transform(&self) -> Transform {
        Transform::from_translation(self.translation.into())
            .with_rotation(Quat::from_array(self.rotation))
    }
}

#[derive(Deserialize)]
struct MaterialSource {
    id: String,
    color: [f32; 4],
    roughness: f32,
    metallic: f32,
    #[serde(default)]
    alpha_mode: MaterialAlpha,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MaterialAlpha {
    #[default]
    Opaque,
    Blend,
}

#[derive(Deserialize, PartialEq)]
struct SurfaceSource {
    material: String,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

impl SurfaceSource {
    fn mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

#[derive(Deserialize, Clone)]
struct Bounds {
    min: [f32; 3],
    max: [f32; 3],
}

#[derive(Deserialize)]
struct PartSource {
    id: String,
    parent: Option<String>,
    pivot: Pose,
    movable: bool,
    meshes: Vec<SurfaceSource>,
    far_meshes: Vec<SurfaceSource>,
    bounds: Bounds,
    lod_deviation: f32,
}

#[derive(Deserialize, Clone)]
struct SocketSource {
    id: String,
    part: String,
    pose: Pose,
}

#[derive(Deserialize)]
struct BakedSource {
    version: u32,
    baker_version: u32,
    id: String,
    source_sha256: String,
    parts: Vec<PartSource>,
    materials: Vec<MaterialSource>,
    sockets: Vec<SocketSource>,
}

struct Surface {
    mesh: Handle<Mesh>,
    far_mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

struct Part {
    id: String,
    parent: Option<String>,
    pivot: Transform,
    movable: bool,
    surfaces: Vec<Surface>,
    bounds: Bounds,
    lod_deviation: f32,
}

struct CachedAsset {
    parts: Vec<Part>,
    sockets: Vec<SocketSource>,
}

/// Strong handles live once per embedded asset for the client session. The
/// finite registry is verified by the build; only requested assets allocate.
#[derive(Default, Resource)]
struct VoxelCache {
    assets: BTreeMap<String, CachedAsset>,
}

impl VoxelCache {
    fn load(
        &mut self,
        id: &str,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Result<&CachedAsset, String> {
        if !self.assets.contains_key(id) {
            let (_, json) = BAKED_ASSETS
                .iter()
                .find(|(name, _)| *name == id)
                .ok_or_else(|| format!("unknown voxel asset '{id}'"))?;
            let source: BakedSource = serde_json::from_str(json)
                .map_err(|error| format!("voxel asset '{id}': {error}"))?;
            // Sources/geometry/hierarchy are checked offline before embedding.
            // Reject incompatible envelopes before allocating any handles.
            if source.version != 1
                || source.baker_version != 2
                || source.id != id
                || source.source_sha256.len() != 64
            {
                return Err(format!("voxel asset '{id}': incompatible bake envelope"));
            }
            let palette: BTreeMap<_, _> = source
                .materials
                .into_iter()
                .map(|m| {
                    (
                        m.id,
                        materials.add(StandardMaterial {
                            base_color: Color::srgba(
                                m.color[0], m.color[1], m.color[2], m.color[3],
                            ),
                            perceptual_roughness: m.roughness,
                            metallic: m.metallic,
                            alpha_mode: match m.alpha_mode {
                                MaterialAlpha::Opaque => AlphaMode::Opaque,
                                MaterialAlpha::Blend => AlphaMode::Blend,
                            },
                            ..default()
                        }),
                    )
                })
                .collect();
            let parts = source
                .parts
                .into_iter()
                .map(|part| {
                    let surfaces = part
                        .meshes
                        .into_iter()
                        .zip(part.far_meshes)
                        .map(|(surface, far)| {
                            let material = palette[&surface.material].clone();
                            let identical = surface == far;
                            let mesh = meshes.add(surface.mesh());
                            let far_mesh = if identical {
                                mesh.clone()
                            } else {
                                meshes.add(far.mesh())
                            };
                            Surface {
                                mesh,
                                far_mesh,
                                material,
                            }
                        })
                        .collect();
                    Part {
                        id: part.id,
                        parent: part.parent,
                        pivot: part.pivot.transform(),
                        movable: part.movable,
                        surfaces,
                        bounds: part.bounds,
                        lod_deviation: part.lod_deviation,
                    }
                })
                .collect();
            self.assets.insert(
                id.to_owned(),
                CachedAsset {
                    parts,
                    sockets: source.sockets,
                },
            );
        }
        Ok(&self.assets[id])
    }
}

fn remove_models(
    mut removed: RemovedComponents<VoxelModel>,
    instances: Query<Option<&VoxelInstance>>,
    mut commands: Commands,
) {
    for entity in removed.read() {
        if let Ok(instance) = instances.get(entity) {
            if let Some(instance) = instance {
                commands.entity(instance.content).despawn();
            }
            commands
                .entity(entity)
                .remove::<(VoxelInstance, VoxelLoadError)>();
        }
    }
}

fn instantiate(
    mut commands: Commands,
    mut cache: ResMut<VoxelCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    models: Query<(Entity, &VoxelModel, Option<&VoxelInstance>), Changed<VoxelModel>>,
) {
    for (root, model, existing) in &models {
        if existing.is_some_and(|instance| instance.source == *model) {
            commands.entity(root).remove::<VoxelLoadError>();
            continue;
        }
        let asset = match cache.load(&model.asset, &mut meshes, &mut materials) {
            Ok(asset) => asset,
            Err(error) => {
                let error = VoxelLoadError(error);
                warn!("{}", error.0);
                commands.entity(root).insert(error);
                continue;
            }
        };
        let layers = if model.view_model {
            RenderLayers::layer(VIEW_MODEL_RENDER_LAYER)
        } else {
            RenderLayers::layer(0)
        };
        let content = commands
            .spawn((
                Name::new(format!("voxel:{}", model.asset)),
                Transform::default(),
                Visibility::Inherited,
            ))
            .id();
        commands.entity(root).add_child(content);
        let mut parts = BTreeMap::new();
        let mut surfaces = Vec::new();
        // Allocate all pivots before linking: declaration order is irrelevant.
        for part in &asset.parts {
            let mut entity = commands.spawn((
                VoxelPart,
                layers.clone(),
                Name::new(part.id.clone()),
                part.pivot,
                Visibility::Inherited,
            ));
            if part.movable {
                entity.insert(MovableVoxelPart);
            }
            parts.insert(part.id.as_str(), entity.id());
        }
        for part in &asset.parts {
            let pivot = parts[part.id.as_str()];
            let parent = part.parent.as_deref().map_or(content, |name| parts[name]);
            commands.entity(parent).add_child(pivot);
            for surface in &part.surfaces {
                let mut entity = commands.spawn((
                    Mesh3d(surface.mesh.clone()),
                    lod::SurfaceLod {
                        near: surface.mesh.clone(),
                        far: surface.far_mesh.clone(),
                        bounds: part.bounds.clone(),
                        deviation: part.lod_deviation,
                    },
                    MeshMaterial3d(surface.material.clone()),
                    layers.clone(),
                    Transform::default(),
                    Visibility::Inherited,
                ));
                if model.view_model {
                    entity.insert(NotShadowCaster);
                }
                let child = entity.id();
                surfaces.push(child);
                commands.entity(pivot).add_child(child);
            }
        }
        for socket in &asset.sockets {
            let entity = commands
                .spawn((
                    VoxelSocket,
                    layers.clone(),
                    Name::new(socket.id.clone()),
                    socket.pose.transform(),
                    Visibility::Inherited,
                ))
                .id();
            commands
                .entity(parts[socket.part.as_str()])
                .add_child(entity);
        }
        if let Some(instance) = existing {
            commands.entity(instance.content).despawn();
        }
        commands
            .entity(root)
            .remove::<VoxelLoadError>()
            .insert(VoxelInstance {
                source: model.clone(),
                content,
                surfaces,
                far: false,
                parts: parts
                    .into_iter()
                    .map(|(name, entity)| (name.to_owned(), entity))
                    .collect(),
            });
    }
}

pub(crate) mod configured;
pub(crate) mod debug;
mod lod;
#[cfg(test)]
mod tests;
