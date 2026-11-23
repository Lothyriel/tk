//! Versioned logical content. No meshes, textures, audio handles or client asset paths.
//! IDs are explicit, append-only numbers, independent of ordering and display names.
use std::{collections::BTreeMap, fmt, sync::OnceLock};

use rkyv::{Archive, Deserialize, Serialize};

mod instance;
pub use instance::*;
mod kit;
pub use kit::*;
mod assembly;
pub use assembly::*;
mod mechanics;
pub use mechanics::*;
pub mod item_grid;
pub mod item_inventory;
pub mod item_manifest;

pub const CATALOG_VERSION: u32 = 2;
pub const CATALOG_FILE: &str = "common/data/catalog.ron";
pub const CATALOG_SOURCE: &str = include_str!("../data/catalog.ron");

macro_rules! id {
    ($name:ident, $storage:ty $(, $derive:ident)*) => {
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Archive,
            Serialize,
            Deserialize,
            serde::Deserialize,
            serde::Serialize,
            $($derive,)*
        )]
        #[serde(transparent)]
        pub struct $name(pub $storage);
    };
}

id!(WeaponId, u32);
id!(DefinitionId, u32);
id!(WeaponInstanceId, u64, Default);
id!(MagazineInstanceId, u64);
id!(ItemInstanceId, u64);
id!(CompartmentId, u64);

impl Default for WeaponId {
    fn default() -> Self {
        Self::M4A1
    }
}

impl WeaponId {
    pub const M4A1: Self = Self(1);
    pub const M1911: Self = Self(2);
    pub const MP5: Self = Self(3);

    /// Only use after validation at the input/content boundary. Unknown IDs never
    /// silently become a different weapon.
    pub fn spec(self) -> &'static ResolvedWeapon {
        catalog().weapon(self).expect("validated weapon definition")
    }

    pub fn class(self) -> WeaponClass {
        self.spec().class
    }
}

pub use crate::WeaponKind as WeaponClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub enum ComponentKind {
    Magazine,
    Stock,
    Handguard,
    Barrel,
    MuzzleDevice,
    Suppressor,
    Compensator,
    Scope,
    RedDot,
    IronSight,
    Grip,
    Adapter,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponDefinition {
    pub id: WeaponId,
    pub key: String,
    pub name: String,
    pub class: WeaponClass,
    pub profile: DefinitionId,
    pub ammo: DefinitionId,
    pub components: Vec<Attachment>,
    pub assembly: WeaponAssembly,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentDefinition {
    pub id: DefinitionId,
    pub key: String,
    pub kind: ComponentKind,
    pub ammo: DefinitionId,
    pub capacity: u32,
    pub mass_kg: f32,
    pub assembly: ComponentAssembly,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmmoDefinition {
    /// Nominal game bore diameter, independent of impact-mark presentation.
    pub bore_diameter_mm: f32,
    pub id: DefinitionId,
    pub key: String,
    pub damage: f32,
    pub penetration: f32,
    pub muzzle_speed: f32,
    /// Effective bullet/gas momentum, N*s; gameplay approximation, not damage.
    pub recoil_impulse_ns: f32,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileDefinition {
    pub id: DefinitionId,
    pub key: String,
    pub rounds_per_minute: f32,
    pub reload_seconds: f32,
    pub automatic: bool,
    /// Shared authoritative muzzle offset during the legacy geometry migration.
    pub barrel_offset: [f32; 3],
    pub impact_size_mm: u8,
    /// Angular transfer gain for the platform's bracing/ergonomics.
    pub recoil_control: f32,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogDocument {
    pub version: u32,
    pub weapons: Vec<WeaponDefinition>,
    pub components: Vec<ComponentDefinition>,
    pub ammo: Vec<AmmoDefinition>,
    pub profiles: Vec<ProfileDefinition>,
}

#[derive(Debug, Clone)]
pub struct ResolvedWeapon {
    pub definition: WeaponId,
    pub catalog_key: String,
    pub name: String,
    pub class: WeaponClass,
    pub ammo: DefinitionId,
    pub magazine_size: u32,
    pub rounds_per_minute: f32,
    pub reload_seconds: f32,
    pub automatic: bool,
    pub damage: f32,
    pub penetration: f32,
    pub muzzle_speed: f32,
    pub bore_diameter_mm: f32,
    pub barrel_offset: [f32; 3],
    pub impact_size_mm: u8,
    pub assembly: ResolvedAssembly,
    pub recoil: crate::recoil::RecoilProfile,
}

impl ResolvedWeapon {
    pub fn seconds_per_shot(&self) -> f32 {
        60.0 / self.rounds_per_minute
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogError {
    pub file: String,
    pub field: String,
    pub message: String,
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.file, self.field, self.message)
    }
}

impl std::error::Error for CatalogError {}

fn error(file: &str, field: impl Into<String>, message: impl Into<String>) -> CatalogError {
    CatalogError {
        file: file.into(),
        field: field.into(),
        message: message.into(),
    }
}

/// Immutable after validation. Items will extend this same document/ID namespace.
#[derive(Debug)]
pub struct Catalog {
    file: String,
    definitions: BTreeMap<WeaponId, WeaponDefinition>,
    components: BTreeMap<DefinitionId, ComponentDefinition>,
    ammo: BTreeMap<DefinitionId, AmmoDefinition>,
    profiles: BTreeMap<DefinitionId, ProfileDefinition>,
    resolved: BTreeMap<WeaponId, ResolvedWeapon>,
}

impl Catalog {
    pub fn parse(file: &str, source: &str) -> Result<Self, CatalogError> {
        let mut deserializer = ron::Deserializer::from_str(source)
            .map_err(|e| error(file, "document", e.to_string()))?;
        let document: CatalogDocument = serde_path_to_error::deserialize(&mut deserializer)
            .map_err(|e| error(file, e.path().to_string(), e.inner().to_string()))?;
        deserializer
            .end()
            .map_err(|e| error(file, "document", e.to_string()))?;
        Self::validate(file, document)
    }

    pub fn validate(file: &str, document: CatalogDocument) -> Result<Self, CatalogError> {
        if document.version != CATALOG_VERSION {
            return Err(error(
                file,
                "version",
                format!("expected {CATALOG_VERSION}, got {}", document.version),
            ));
        }
        let mut ids = BTreeMap::new();
        let mut keys = BTreeMap::new();
        for (group, entries) in [
            (
                "weapons",
                document
                    .weapons
                    .iter()
                    .map(|d| (d.id.0, d.key.as_str()))
                    .collect::<Vec<_>>(),
            ),
            (
                "components",
                document
                    .components
                    .iter()
                    .map(|d| (d.id.0, d.key.as_str()))
                    .collect(),
            ),
            (
                "ammo",
                document
                    .ammo
                    .iter()
                    .map(|d| (d.id.0, d.key.as_str()))
                    .collect(),
            ),
            (
                "profiles",
                document
                    .profiles
                    .iter()
                    .map(|d| (d.id.0, d.key.as_str()))
                    .collect(),
            ),
        ] {
            for (i, (id, key)) in entries.into_iter().enumerate() {
                let field = format!("{group}[{i}]");
                if id == 0 {
                    return Err(error(file, format!("{field}.id"), "zero is reserved"));
                }
                if let Some(previous) = ids.insert(id, field.clone()) {
                    return Err(error(
                        file,
                        format!("{field}.id"),
                        format!("duplicate ID {id}, already in {previous}"),
                    ));
                }
                if key.is_empty()
                    || !key
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
                {
                    return Err(error(
                        file,
                        format!("{field}.key"),
                        "expected lowercase ASCII key",
                    ));
                }
                if let Some(previous) = keys.insert(key, field.clone()) {
                    return Err(error(
                        file,
                        format!("{field}.key"),
                        format!("duplicate key, already in {previous}"),
                    ));
                }
            }
        }
        for (i, ammo) in document.ammo.iter().enumerate() {
            if !ammo.recoil_impulse_ns.is_finite()
                || !(0.01..=100.0).contains(&ammo.recoil_impulse_ns)
            {
                return Err(error(
                    file,
                    format!("ammo[{i}].recoil_impulse_ns"),
                    "expected 0.01..=100 N*s",
                ));
            }
            for (field, value) in [
                ("bore_diameter_mm", ammo.bore_diameter_mm),
                ("damage", ammo.damage),
                ("penetration", ammo.penetration),
                ("muzzle_speed", ammo.muzzle_speed),
            ] {
                positive(file, format!("ammo[{i}].{field}"), value)?;
            }
        }
        for (i, profile) in document.profiles.iter().enumerate() {
            if !profile.recoil_control.is_finite()
                || !(0.01..=10.0).contains(&profile.recoil_control)
            {
                return Err(error(
                    file,
                    format!("profiles[{i}].recoil_control"),
                    "expected 0.01..=10 gain",
                ));
            }
            positive(
                file,
                format!("profiles[{i}].rounds_per_minute"),
                profile.rounds_per_minute,
            )?;
            positive(
                file,
                format!("profiles[{i}].reload_seconds"),
                profile.reload_seconds,
            )?;
            if !profile.barrel_offset.iter().all(|v| v.is_finite()) {
                return Err(error(
                    file,
                    format!("profiles[{i}].barrel_offset"),
                    "expected finite coordinates",
                ));
            }
            if profile.impact_size_mm == 0 {
                return Err(error(
                    file,
                    format!("profiles[{i}].impact_size_mm"),
                    "expected positive size",
                ));
            }
        }
        for (i, component) in document.components.iter().enumerate() {
            positive(file, format!("components[{i}].mass_kg"), component.mass_kg)?;
            if (component.kind == ComponentKind::Magazine && component.capacity == 0)
                || component.capacity > 1000
                || (component.kind != ComponentKind::Magazine && component.capacity != 0)
            {
                return Err(error(
                    file,
                    format!("components[{i}].capacity"),
                    "expected 1..=1000 rounds",
                ));
            }
            if (component.kind == ComponentKind::Magazine || component.ammo.0 != 0)
                && !document.ammo.iter().any(|a| a.id == component.ammo)
            {
                return Err(error(
                    file,
                    format!("components[{i}].ammo"),
                    "unknown ammo ID",
                ));
            }
        }
        assembly::validate::document(file, &document)?;
        let mut catalog = Self {
            file: file.into(),
            definitions: BTreeMap::new(),
            components: document.components.into_iter().map(|d| (d.id, d)).collect(),
            ammo: document.ammo.into_iter().map(|d| (d.id, d)).collect(),
            profiles: document.profiles.into_iter().map(|d| (d.id, d)).collect(),
            resolved: BTreeMap::new(),
        };
        for (i, definition) in document.weapons.into_iter().enumerate() {
            let field = format!("weapons[{i}]");
            if definition.name.trim().is_empty() {
                return Err(error(
                    file,
                    format!("{field}.name"),
                    "expected display name",
                ));
            }
            let resolved =
                catalog.resolve_definition(&definition, &definition.components, &field)?;
            catalog.resolved.insert(definition.id, resolved);
            catalog.definitions.insert(definition.id, definition);
        }
        Ok(catalog)
    }

    fn resolve_definition(
        &self,
        definition: &WeaponDefinition,
        components: &[Attachment],
        field: &str,
    ) -> Result<ResolvedWeapon, CatalogError> {
        let profile = self
            .profiles
            .get(&definition.profile)
            .ok_or_else(|| error(&self.file, format!("{field}.profile"), "unknown profile ID"))?;
        let ammo = self
            .ammo
            .get(&definition.ammo)
            .ok_or_else(|| error(&self.file, format!("{field}.ammo"), "unknown ammo ID"))?;
        let (assembly, capacity) = self.resolve_assembly(definition, components, field)?;
        let barrel_offset =
            assembly
                .anchors
                .get(&AnchorKind::Bore)
                .map_or(profile.barrel_offset, |pose| {
                    assembly
                        .hip_from_root
                        .transform_point(pose.translation)
                        .to_array()
                });
        let muzzle_speed = ammo.muzzle_speed * assembly.velocity_multiplier;
        if !muzzle_speed.is_finite() || !(1.0..=5000.0).contains(&muzzle_speed) {
            return Err(error(
                &self.file,
                format!("{field}.muzzle_speed"),
                "resolved velocity outside 1..=5000 m/s",
            ));
        }
        Ok(ResolvedWeapon {
            definition: definition.id,
            catalog_key: definition.key.clone(),
            name: definition.name.clone(),
            class: definition.class,
            ammo: ammo.id,
            magazine_size: capacity,
            rounds_per_minute: profile.rounds_per_minute,
            reload_seconds: profile.reload_seconds,
            automatic: profile.automatic,
            damage: ammo.damage,
            penetration: ammo.penetration,
            muzzle_speed,
            bore_diameter_mm: ammo.bore_diameter_mm,
            barrel_offset,
            impact_size_mm: profile.impact_size_mm,
            recoil: crate::recoil::RecoilProfile::resolve(
                &assembly,
                ammo.recoil_impulse_ns,
                profile.recoil_control,
            ),
            assembly,
        })
    }

    pub fn weapon(&self, id: WeaponId) -> Option<&ResolvedWeapon> {
        self.resolved.get(&id)
    }

    pub fn weapons(&self) -> impl Iterator<Item = &ResolvedWeapon> {
        self.resolved.values()
    }

    pub fn definition(&self, id: WeaponId) -> Option<&WeaponDefinition> {
        self.definitions.get(&id)
    }

    pub fn component(&self, id: DefinitionId) -> Option<&ComponentDefinition> {
        self.components.get(&id)
    }

    pub fn ammo(&self, id: DefinitionId) -> Option<&AmmoDefinition> {
        self.ammo.get(&id)
    }

    pub fn profile(&self, id: DefinitionId) -> Option<&ProfileDefinition> {
        self.profiles.get(&id)
    }

    pub fn resolve(
        &self,
        configuration: &WeaponConfiguration,
    ) -> Result<ResolvedWeapon, CatalogError> {
        let definition = self
            .definition(configuration.definition)
            .ok_or_else(|| error(&self.file, "configuration.definition", "unknown weapon ID"))?;
        self.resolve_definition(definition, &configuration.components, "configuration")
    }

    pub fn configuration(&self, id: WeaponId) -> Result<WeaponConfiguration, CatalogError> {
        let definition = self
            .definition(id)
            .ok_or_else(|| error(&self.file, "configuration.definition", "unknown weapon ID"))?;
        Ok(WeaponConfiguration {
            definition: id,
            components: definition.components.clone(),
        })
    }
}

fn positive(file: &str, field: String, value: f32) -> Result<(), CatalogError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(error(file, field, "expected finite positive value"));
    }
    Ok(())
}

pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        Catalog::parse(CATALOG_FILE, CATALOG_SOURCE).expect("valid bundled logical catalog")
    })
}

#[cfg(test)]
mod tests;
