use super::{planned_items, S1_SEEDS};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

pub const ITEM_VISUALS_FILE: &str = "common/data/item-visuals.ron";
const SOURCE: &str = include_str!("../../../data/item-visuals.ron");

#[derive(Debug, Deserialize)]
pub struct ItemVisual {
    pub asset_key: String,
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl ItemVisual {
    pub fn collider_center(&self) -> [f32; 3] {
        std::array::from_fn(|axis| (self.max[axis] + self.min[axis]) * 0.5)
    }

    pub fn collider_half_extents(&self) -> [f32; 3] {
        std::array::from_fn(|axis| (self.max[axis] - self.min[axis]) * 0.5)
    }
}

pub fn item_visuals() -> &'static BTreeMap<String, ItemVisual> {
    static VISUALS: OnceLock<BTreeMap<String, ItemVisual>> = OnceLock::new();
    VISUALS.get_or_init(|| parse(SOURCE).expect("valid item visual bindings"))
}

fn parse(source: &str) -> Result<BTreeMap<String, ItemVisual>, String> {
    let entries: BTreeMap<String, ItemVisual> =
        ron::from_str(source).map_err(|error| format!("{ITEM_VISUALS_FILE}: {error}"))?;
    for id in S1_SEEDS {
        if !entries.contains_key(id) {
            return Err(format!("{ITEM_VISUALS_FILE}: {id}: missing seed binding"));
        }
    }
    for (id, visual) in &entries {
        let valid_bounds = visual.min.into_iter().zip(visual.max).all(|(min, max)| {
            min.is_finite() && max.is_finite() && min >= -8.0 && max <= 8.0 && min < max
        });
        if !planned_items().contains_key(id.as_str())
            || visual.asset_key.trim().is_empty()
            || !valid_bounds
        {
            return Err(format!(
                "{ITEM_VISUALS_FILE}: {id}: invalid ID, asset key or bounds"
            ));
        }
    }
    Ok(entries)
}
