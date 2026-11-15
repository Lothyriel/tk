//! Rigid attachment contracts. Distances are metres, mass kilograms, rotations
//! unit quaternions (xyzw). Configuration carries IDs/placement, never stats.
use super::*;
use bevy::prelude::{Quat, Transform, Vec3};

mod authored;
mod resolve;
pub use authored::{authored_rig, AuthoredRig, LOGICAL_RIGS_SOURCE};
#[cfg(test)]
mod tests;
pub(super) mod validate;

pub const MAX_COMPONENTS: usize = 32;
pub const MAX_DEPTH: usize = 8;
pub const MAX_SOCKETS: usize = 16;
pub const MAX_VOLUMES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub enum InterfaceKind {
    Magwell,
    Rail,
    Thread,
    Dovetail,
    Platform,
    Mount,
}

/// Exact standard AND size match, e.g. Thread/"1/2-28"/5560.
/// Size is a standard-specific integer code, not an inferred dimension.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    pub kind: InterfaceKind,
    pub standard: String,
    pub size: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigidPose {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
}

impl Default for RigidPose {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

impl RigidPose {
    pub fn transform(self) -> Transform {
        Transform::from_translation(Vec3::from(self.translation))
            .with_rotation(Quat::from_array(self.rotation).normalize())
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Socket {
    pub key: String,
    pub interface: Interface,
    pub accepts: Vec<ComponentKind>,
    #[serde(default)]
    pub pose: RigidPose,
    /// One slot for rigid sockets; multiple indexed slots for a rail.
    pub positions: u16,
    /// Rail positions advance along local +Z.
    pub pitch_m: f32,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Bounds {
    fn transformed(self, transform: Transform) -> Self {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for x in [self.min[0], self.max[0]] {
            for y in [self.min[1], self.max[1]] {
                for z in [self.min[2], self.max[2]] {
                    let p = transform.transform_point(Vec3::new(x, y, z));
                    min = min.min(p);
                    max = max.max(p);
                }
            }
        }
        Self {
            min: min.to_array(),
            max: max.to_array(),
        }
    }

    fn overlaps(self, other: Self) -> bool {
        (0..3).all(|i| self.min[i] < other.max[i] && other.min[i] < self.max[i])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize)]
pub enum AnchorKind {
    Bore,
    DeviceExit,
    Ejection,
    TriggerGrip,
    SupportGrip,
    Sight,
    FrontIron,
    RearIron,
    StockContact,
    MagazineGrip,
    ControlGrip,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub kind: AnchorKind,
    pub pose: RigidPose,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(try_from = "authored::GeometrySource")]
pub struct Geometry {
    pub center_of_mass: [f32; 3],
    pub sockets: Vec<Socket>,
    /// Conservative non-contact solids; author intentional mating areas outside
    /// these boxes. They are not render meshes or Rapier collision bodies.
    pub solids: Vec<Bounds>,
    /// Swept action/sight corridors which other parts may not occupy.
    pub clearance: Vec<Bounds>,
    pub anchors: Vec<Anchor>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponAssembly {
    pub platform: String,
    pub caliber: String,
    pub mass_kg: f32,
    pub geometry: Geometry,
    /// Explicit bridge from breech-root assets to the legacy eye-relative rig.
    #[serde(default)]
    pub hip_from_root: RigidPose,
    #[serde(default)]
    pub ads_from_root: RigidPose,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentAssembly {
    pub interface: Interface,
    pub platform: Option<String>,
    pub caliber: Option<String>,
    pub occupied_positions: u16,
    pub geometry: Geometry,
    #[serde(default)]
    pub requires: Vec<DefinitionId>,
    #[serde(default)]
    pub excludes: Vec<DefinitionId>,
    #[serde(default)]
    pub modifiers: Modifiers,
}

/// Explicit dimensionless multipliers. No behavior is inferred from part kind.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Modifiers {
    pub recoil: f32,
    pub velocity: f32,
    pub signature: f32,
}

impl Default for Modifiers {
    fn default() -> Self {
        Self {
            recoil: 1.0,
            velocity: 1.0,
            signature: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Archive, Serialize, Deserialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    /// Stable within this configuration; zero is reserved for the weapon root.
    pub node: u16,
    pub definition: DefinitionId,
    pub parent: Option<u16>,
    pub socket: String,
    pub position: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPart {
    pub node: u16,
    pub definition: DefinitionId,
    pub kind: super::ComponentKind,
    pub pose: Transform,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedAssembly {
    pub parts: Vec<ResolvedPart>,
    pub mass_kg: f32,
    pub center_of_mass: [f32; 3],
    /// Static first moment around the root, kg*m. Consumers tune handling later.
    pub balance_kg_m: f32,
    pub recoil_multiplier: f32,
    pub velocity_multiplier: f32,
    pub signature_multiplier: f32,
    pub anchors: BTreeMap<AnchorKind, Transform>,
    pub obstruction: Vec<Bounds>,
    pub hip_from_root: Transform,
    pub ads_from_root: Transform,
}
