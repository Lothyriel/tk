use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_MAGAZINE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub enum EquipmentSlot {
    #[default]
    Primary,
    Sidearm,
}

#[derive(Debug, Clone, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct WeaponConfiguration {
    pub definition: WeaponId,
    pub components: Vec<Attachment>,
}

/// Persistent identity and mutable state belong to one physical weapon, not a
/// class or slot. Inventory will reference this instance rather than copy it.
#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
pub struct WeaponInstance {
    pub id: WeaponInstanceId,
    pub configuration: WeaponConfiguration,
    pub mechanics: MechanicalState,
    pub revision: u64,
}

/// Configuration resolved once on equip/reconfigure, never in the frame loop.
#[derive(Debug, Clone)]
pub struct EquippedWeapon {
    instance: WeaponInstance,
    resolved: ResolvedWeapon,
    magazine_variants: BTreeMap<DefinitionId, ResolvedWeapon>,
    magazine_node: u16,
}

impl EquippedWeapon {
    pub fn new(
        catalog: &Catalog,
        id: WeaponInstanceId,
        configuration: WeaponConfiguration,
    ) -> Result<Self, CatalogError> {
        if id.0 == 0 {
            return Err(error(&catalog.file, "instance.id", "zero is reserved"));
        }
        let resolved = catalog.resolve(&configuration)?;
        let first = NEXT_MAGAZINE
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(MAX_RETAINED_MAGAZINES as u64)
            })
            .map_err(|_| error(&catalog.file, "magazine.id", "identity exhausted"))?;
        let ids = std::array::from_fn(|i| MagazineInstanceId(first + i as u64));
        let mechanics = MechanicalState::new(catalog, &resolved, ids)?;
        Self::restore(
            catalog,
            WeaponInstance {
                id,
                configuration,
                mechanics,
                revision: 0,
            },
        )
    }

    pub fn restore(catalog: &Catalog, instance: WeaponInstance) -> Result<Self, CatalogError> {
        if instance.id.0 == 0 {
            return Err(error(&catalog.file, "instance.id", "zero is reserved"));
        }
        let resolved = catalog.resolve(&instance.configuration)?;
        instance.mechanics.validate(catalog, &resolved)?;
        let (magazine_node, magazine_variants) =
            Self::variants(catalog, &instance.configuration, &instance.mechanics)?;
        if instance.mechanics.inserted().is_some_and(|m| {
            instance
                .configuration
                .components
                .iter()
                .find(|n| n.node == magazine_node)
                .unwrap()
                .definition
                != m.definition
        }) {
            return Err(error(
                &catalog.file,
                "instance.magazine",
                "configuration and inserted magazine disagree",
            ));
        }
        let next = instance
            .mechanics
            .magazines()
            .iter()
            .map(|m| m.id.0)
            .max()
            .unwrap()
            .checked_add(1)
            .ok_or_else(|| error(&catalog.file, "magazine.id", "identity exhausted"))?;
        NEXT_MAGAZINE.fetch_max(next, Ordering::Relaxed);
        Ok(Self {
            instance,
            resolved,
            magazine_variants,
            magazine_node,
        })
    }

    fn variants(
        catalog: &Catalog,
        configuration: &WeaponConfiguration,
        mechanics: &MechanicalState,
    ) -> Result<(u16, BTreeMap<DefinitionId, ResolvedWeapon>), CatalogError> {
        let magazine_node = configuration
            .components
            .iter()
            .find(|n| {
                catalog
                    .component(n.definition)
                    .is_some_and(|c| c.kind == ComponentKind::Magazine)
            })
            .ok_or_else(|| {
                error(
                    &catalog.file,
                    "configuration.magazine",
                    "missing magazine node",
                )
            })?
            .node;
        let mut variants = BTreeMap::new();
        for magazine in mechanics.magazines() {
            if variants.contains_key(&magazine.definition) {
                continue;
            }
            let mut candidate = configuration.clone();
            candidate
                .components
                .iter_mut()
                .find(|n| n.node == magazine_node)
                .unwrap()
                .definition = magazine.definition;
            variants.insert(magazine.definition, catalog.resolve(&candidate)?);
        }
        Ok((magazine_node, variants))
    }

    pub fn instance(&self) -> &WeaponInstance {
        &self.instance
    }

    pub fn spec(&self) -> &ResolvedWeapon {
        &self.resolved
    }

    /// Compare-and-swap of the entire validated configuration. No partial
    /// removal, client-supplied stats or ammunition truncation is possible.
    pub fn reconfigure(
        &mut self,
        catalog: &Catalog,
        expected_revision: u64,
        mut configuration: WeaponConfiguration,
    ) -> Result<u64, CatalogError> {
        if expected_revision != self.instance.revision {
            return Err(error(
                &catalog.file,
                "instance.revision",
                "stale configuration revision",
            ));
        }
        if configuration.definition != self.instance.configuration.definition {
            return Err(error(
                &catalog.file,
                "configuration.definition",
                "cannot replace instance identity",
            ));
        }
        configuration.components.sort_by_key(|node| node.node);
        let mut previous = self.instance.configuration.clone();
        previous.components.sort_by_key(|node| node.node);
        if configuration == previous {
            return Ok(self.instance.revision);
        }
        let revision = expected_revision
            .checked_add(1)
            .ok_or_else(|| error(&catalog.file, "instance.revision", "revision exhausted"))?;
        let resolved = catalog.resolve(&configuration)?;
        let (magazine_node, variants) =
            Self::variants(catalog, &configuration, &self.instance.mechanics)?;
        let magazine = configuration
            .components
            .iter()
            .find(|n| n.node == magazine_node)
            .unwrap()
            .definition;
        let mut mechanics = self.instance.mechanics.clone();
        mechanics.select_magazine(magazine)?;
        mechanics.validate(catalog, &resolved)?;
        self.instance.configuration = configuration;
        self.instance.revision = revision;
        self.instance.mechanics = mechanics;
        self.resolved = resolved;
        self.magazine_node = magazine_node;
        self.magazine_variants = variants;
        Ok(revision)
    }

    pub fn rounds(&self) -> u32 {
        self.instance.mechanics.loaded_rounds()
    }

    pub fn reserve(&self) -> u32 {
        self.instance.mechanics.reserve_rounds()
    }

    /// Loading/restoring state validates capacity before exposing mutable ammo.
    pub fn set_ammunition(&mut self, magazine: u32, reserve: u32) -> Result<(), CatalogError> {
        self.instance.mechanics.set_ammunition(magazine, reserve)
    }

    pub fn fire(&mut self) -> bool {
        self.instance.mechanics.fire(true, true)
    }

    pub fn mechanics(&self) -> &MechanicalState {
        &self.instance.mechanics
    }
    pub fn mechanics_mut(&mut self) -> &mut MechanicalState {
        &mut self.instance.mechanics
    }

    pub fn advance(&mut self, delta: f32) {
        // A new magazine definition changes the configuration revision. Never
        // commit it under a reused maximum revision.
        if self.instance.revision == u64::MAX
            && self
                .instance
                .mechanics
                .action()
                .and_then(|a| a.target_magazine)
                .and_then(|id| {
                    self.instance
                        .mechanics
                        .magazines()
                        .iter()
                        .find(|m| m.id == id)
                })
                .is_some_and(|m| {
                    self.instance
                        .configuration
                        .components
                        .iter()
                        .any(|n| n.node == self.magazine_node && n.definition != m.definition)
                })
        {
            self.instance.mechanics.cancel();
            return;
        }
        self.instance.mechanics.advance(delta);
        if let Some(magazine) = self.instance.mechanics.inserted() {
            let node = self
                .instance
                .configuration
                .components
                .iter_mut()
                .find(|n| n.node == self.magazine_node)
                .unwrap();
            if node.definition != magazine.definition {
                node.definition = magazine.definition;
                self.resolved = self.magazine_variants[&magazine.definition].clone();
                self.instance.revision += 1;
            }
        }
    }
}
