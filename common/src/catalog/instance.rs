use super::*;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub enum EquipmentSlot {
    #[default]
    Primary,
    Sidearm,
}

#[derive(Debug, Clone, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct WeaponConfiguration {
    pub definition: WeaponId,
    pub components: Vec<DefinitionId>,
}

/// Persistent identity and mutable state belong to one physical weapon, not a
/// class or slot. Inventory will reference this instance rather than copy it.
#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
pub struct WeaponInstance {
    pub id: WeaponInstanceId,
    pub configuration: WeaponConfiguration,
    pub magazine: u32,
    pub reserve: u32,
}

/// Configuration resolved once on equip/reconfigure, never in the frame loop.
#[derive(Debug, Clone)]
pub struct EquippedWeapon {
    instance: WeaponInstance,
    resolved: ResolvedWeapon,
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
        Ok(Self {
            instance: WeaponInstance {
                id,
                configuration,
                magazine: resolved.magazine_size,
                reserve: resolved.magazine_size * 3,
            },
            resolved,
        })
    }

    pub fn instance(&self) -> &WeaponInstance {
        &self.instance
    }

    pub fn spec(&self) -> &ResolvedWeapon {
        &self.resolved
    }

    pub fn rounds(&self) -> u32 {
        self.instance.magazine
    }

    pub fn reserve(&self) -> u32 {
        self.instance.reserve
    }

    /// Loading/restoring state validates capacity before exposing mutable ammo.
    pub fn set_ammunition(&mut self, magazine: u32, reserve: u32) -> Result<(), CatalogError> {
        if magazine > self.resolved.magazine_size {
            return Err(error(
                CATALOG_FILE,
                "instance.magazine",
                "exceeds resolved capacity",
            ));
        }
        self.instance.magazine = magazine;
        self.instance.reserve = reserve;
        Ok(())
    }

    pub fn fire(&mut self) -> bool {
        if let Some(rounds) = self.instance.magazine.checked_sub(1) {
            self.instance.magazine = rounds;
            true
        } else {
            false
        }
    }

    pub fn reload(&mut self) {
        let count =
            (self.resolved.magazine_size - self.instance.magazine).min(self.instance.reserve);
        self.instance.magazine += count;
        self.instance.reserve -= count;
    }
}
