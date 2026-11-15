//! A free raid kit is a selection, not physical inventory. Only materializing
//! accepted initial equipment allocates magazines; reconfiguration never does.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct WeaponKit {
    pub weapon: WeaponConfiguration,
    /// Reserve definitions, in selection order. The inserted magazine belongs
    /// to the assembly; together they fit the existing physical magazine pool.
    pub reserves: Vec<DefinitionId>,
}

impl WeaponKit {
    pub fn from_configuration(
        catalog: &Catalog,
        weapon: WeaponConfiguration,
    ) -> Result<Self, CatalogError> {
        catalog.resolve(&weapon)?;
        let magazine = Self::magazine(catalog, &weapon)?;
        let reserves = vec![magazine.definition; MAX_RETAINED_MAGAZINES - 1];
        Ok(Self { weapon, reserves })
    }

    pub fn resolve(&self, catalog: &Catalog) -> Result<ResolvedWeapon, CatalogError> {
        if self.reserves.len() >= MAX_RETAINED_MAGAZINES {
            return Err(error(
                &catalog.file,
                "kit.reserves",
                "too many reserve magazines",
            ));
        }
        let resolved = catalog.resolve(&self.weapon)?;
        let magazine = Self::magazine(catalog, &self.weapon)?;
        // Resolve each reserve in the actual assembly, not just against ammo
        // or interface tags: its envelope can conflict with installed parts.
        for definition in &self.reserves {
            if !catalog
                .component(*definition)
                .is_some_and(|part| part.kind == ComponentKind::Magazine)
            {
                return Err(error(
                    &catalog.file,
                    "kit.reserves",
                    "reserve is not a magazine definition",
                ));
            }
            let mut candidate = self.weapon.clone();
            candidate
                .components
                .iter_mut()
                .find(|part| part.node == magazine.node)
                .unwrap()
                .definition = *definition;
            catalog.resolve(&candidate)?;
        }
        Ok(resolved)
    }

    pub(super) fn magazine<'a>(
        catalog: &Catalog,
        weapon: &'a WeaponConfiguration,
    ) -> Result<&'a Attachment, CatalogError> {
        weapon
            .components
            .iter()
            .find(|part| {
                catalog
                    .component(part.definition)
                    .is_some_and(|part| part.kind == ComponentKind::Magazine)
            })
            .ok_or_else(|| error(&catalog.file, "kit.magazine", "missing inserted magazine"))
    }
}

#[cfg(test)]
mod tests;
