//! Explicit promotion of planned rows with assets from the visual catalog.
use super::{planned_items, PlannedItem};
use std::{collections::BTreeMap, fmt};

#[derive(Clone, Copy, Debug)]
pub struct ItemAssetBinding<'a> {
    pub id: &'a str,
    /// Logical key, resolved externally; common never opens client asset paths.
    pub asset_key: &'a str,
}

#[derive(Debug)]
pub struct ActiveItem<T> {
    pub definition: &'static PlannedItem<'static>,
    pub asset: T,
}

#[derive(Debug)]
pub struct ActiveItems<T>(BTreeMap<&'static str, ActiveItem<T>>);

#[derive(Debug, PartialEq, Eq)]
pub struct ItemActivationError {
    pub source: String,
    pub id: String,
    pub field: &'static str,
    pub detail: String,
}

impl fmt::Display for ItemActivationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}: {}: {}",
            self.source, self.id, self.field, self.detail
        )
    }
}

impl std::error::Error for ItemActivationError {}

impl<T> ActiveItems<T> {
    /// Publish the explicit subset only if all bindings resolve. Pass the
    /// lookup in the validated visual catalog, not a fallback asset loader.
    /// This neither spawns items nor grants effects or inventory actions.
    pub fn validate<'a>(
        source: &str,
        bindings: impl IntoIterator<Item = ItemAssetBinding<'a>>,
        mut resolve: impl FnMut(&str) -> Option<T>,
    ) -> Result<Self, ItemActivationError> {
        let mut items = BTreeMap::new();
        for ItemAssetBinding { id, asset_key } in bindings {
            let error = |field, detail| ItemActivationError {
                source: source.into(),
                id: id.into(),
                field,
                detail,
            };
            let definition = planned_items()
                .get(id)
                .ok_or_else(|| error("id", format!("not defined in {}", super::FILE)))?;
            if items.contains_key(id) {
                return Err(error("id", "duplicate binding".into()));
            }
            let asset = (!asset_key.trim().is_empty())
                .then(|| resolve(asset_key))
                .flatten()
                .ok_or_else(|| error("asset_key", format!("unresolved asset {asset_key:?}")))?;
            items.insert(definition.id, ActiveItem { definition, asset });
        }
        Ok(Self(items))
    }

    pub fn get(&self, id: &str) -> Option<&ActiveItem<T>> {
        self.0.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "fixture/item-assets";
    const BANDAGE: ItemAssetBinding<'_> = ItemAssetBinding {
        id: "med.bandage",
        asset_key: "bandage_model",
    };

    #[test]
    fn item_activation_requires_explicit_resolved_assets_but_not_new_id_logic() {
        let alternative = ItemAssetBinding {
            id: "med.pressure_dressing",
            asset_key: "pressure_model",
        };
        let visuals = BTreeMap::from([("bandage_model", 7), ("pressure_model", 13)]);
        let validate =
            |bindings| ActiveItems::validate(SOURCE, bindings, |key| visuals.get(key).copied());
        let active = validate(vec![BANDAGE, alternative]).unwrap();
        let bandage = active.get(BANDAGE.id).unwrap();
        let pressure = active.get(alternative.id).unwrap();
        assert_eq!(bandage.definition.profile, pressure.definition.profile);
        assert_eq!((bandage.asset, pressure.asset), (7, 13));
        assert!(active.get("med.tourniquet").is_none());
        assert!(active.get("food.water500").is_none());
        assert!(validate(vec![]).unwrap().get(BANDAGE.id).is_none());
    }

    #[test]
    fn item_activation_diagnoses_duplicate_unknown_and_missing_asset() {
        for (binding, field, detail) in [
            (BANDAGE, "id", "duplicate"),
            (
                ItemAssetBinding {
                    id: "med.unknown",
                    ..BANDAGE
                },
                "id",
                super::super::FILE,
            ),
            (
                ItemAssetBinding {
                    id: "food.water500",
                    asset_key: "absent_model",
                },
                "asset_key",
                "absent_model",
            ),
            (
                ItemAssetBinding {
                    id: "food.water500",
                    asset_key: " ",
                },
                "asset_key",
                "unresolved",
            ),
        ] {
            let error = ActiveItems::validate(SOURCE, [BANDAGE, binding], |key| {
                (key == BANDAGE.asset_key).then_some(7)
            })
            .unwrap_err();
            assert_eq!(
                (&*error.source, &*error.id, error.field),
                (SOURCE, binding.id, field)
            );
            assert!(error.to_string().contains(detail), "{error}");
        }
    }
}
