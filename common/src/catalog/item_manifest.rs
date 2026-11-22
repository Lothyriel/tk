//! Planned inventory metadata. A row is not an enabled gameplay item.
use super::item_grid::Footprint;
use std::collections::{BTreeMap, BTreeSet};

mod activation;
pub use activation::{ActiveItem, ActiveItems, ItemActivationError, ItemAssetBinding};

pub const FILE: &str = "common/data/items-v01.tsv";
pub const REVISION: &str = "0.1";
const SOURCE: &str = include_str!("../../data/items-v01.tsv");
const HEADER: &str =
    "id\tnome\tgrade\tmassa_g\tstack_max\tmaior_dim_cm\tperfil\tfinalidade\tdirecao_visual";

pub const S1_SEEDS: [&str; 6] = [
    "med.bandage",
    "ammo.mag_ar30",
    "food.water500",
    "elec.ssd",
    "value.watch_mechanical",
    "carry.daypack",
];

#[derive(Debug, PartialEq, Eq)]
pub struct PlannedItem<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub footprint: Footprint,
    pub mass_g: u32,
    pub stack_max: u16,
    pub longest_dimension_cm: u16,
    pub profile: &'a str,
    pub purpose: &'a str,
    pub art_direction: &'a str,
}

pub fn planned_items() -> &'static BTreeMap<&'static str, PlannedItem<'static>> {
    static ITEMS: std::sync::OnceLock<BTreeMap<&str, PlannedItem<'_>>> = std::sync::OnceLock::new();
    ITEMS.get_or_init(|| parse(SOURCE).expect("valid versioned item manifest"))
}

fn valid_id(id: &str) -> bool {
    let Some((category, key)) = id.split_once('.') else {
        return false;
    };
    !category.is_empty()
        && category.bytes().all(|b| b.is_ascii_lowercase())
        && !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn positive<T: std::str::FromStr + Default + PartialOrd>(text: &str) -> Option<T> {
    text.parse().ok().filter(|value| *value > T::default())
}

fn parse(source: &str) -> Result<BTreeMap<&str, PlannedItem<'_>>, String> {
    let mut lines = source.lines();
    if lines.next() != Some(HEADER) {
        return Err(format!("{FILE}:1: unexpected header"));
    }
    let mut items = BTreeMap::new();
    let mut variants = BTreeMap::new();
    for (index, line) in lines.enumerate() {
        let fields: Vec<_> = line.split('\t').collect();
        let fail = |field| format!("{FILE}:{}: {}: invalid {field}", index + 2, fields[0]);
        let [id, name, grid, mass, stack, longest, profile, purpose, art] = fields[..] else {
            return Err(fail("column count"));
        };
        if !valid_id(id) || items.contains_key(id) {
            return Err(fail("id (malformed or duplicate)"));
        }
        for (field, value) in [("nome", name), ("perfil", profile), ("finalidade", purpose)] {
            if value.trim().is_empty() {
                return Err(fail(field));
            }
        }
        let footprint = grid
            .split_once('x')
            .and_then(|(w, h)| Footprint::new(w.parse().ok()?, h.parse().ok()?))
            .ok_or_else(|| fail("grade"))?;
        if let Some(variant) = art.strip_prefix("V:") {
            let (base, _) = variant
                .split_once(' ')
                .filter(|(_, description)| !description.trim().is_empty())
                .ok_or_else(|| fail("direcao_visual"))?;
            let base = if base.contains('.') {
                base.to_owned()
            } else {
                format!("{}.{}", id.split_once('.').unwrap().0, base)
            };
            variants.insert(id, base);
        } else if !art
            .strip_prefix("U ")
            .is_some_and(|text| !text.trim().is_empty())
        {
            return Err(fail("direcao_visual"));
        }
        items.insert(
            id,
            PlannedItem {
                id,
                name,
                footprint,
                mass_g: positive(mass).ok_or_else(|| fail("massa_g"))?,
                stack_max: positive(stack).ok_or_else(|| fail("stack_max"))?,
                longest_dimension_cm: positive(longest).ok_or_else(|| fail("maior_dim_cm"))?,
                profile,
                purpose,
                art_direction: art,
            },
        );
    }
    for &id in variants.keys() {
        let mut seen = BTreeSet::new();
        let mut current = id;
        while let Some(base) = variants.get(current) {
            if !seen.insert(current) || !items.contains_key(base.as_str()) {
                return Err(format!(
                    "{FILE}: {id}: direcao_visual: missing base or cycle at {base}"
                ));
            }
            current = base;
        }
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_manifest_preserves_all_planned_ids_and_s1_seeds() {
        let items = planned_items();
        assert_eq!(items.len(), 270);
        for seed in S1_SEEDS {
            assert_eq!(items[seed].id, seed);
        }
        assert_eq!(items["med.bandage"].stack_max, 3);
        assert_eq!(items["food.water500"].footprint.dimensions(), [1, 2]);
    }

    #[test]
    fn item_manifest_reports_duplicate_dimensions_and_bad_variant_links() {
        let row = "food.water\tWater\t1x2\t530\t1\t21\tfood\tPurpose\tU Shape";
        for (rows, message) in [
            (format!("{row}\n{row}"), "id (malformed or duplicate)"),
            (row.replace("1x2", "0x2"), "grade"),
            (row.replace("530", "NaN"), "massa_g"),
            (row.replace("530", "-1"), "massa_g"),
            (row.replace("U Shape", "V:missing Shape"), "missing base"),
            (row.replace("U Shape", "V:water Shape"), "cycle"),
        ] {
            let error = parse(&format!("{HEADER}\n{rows}\n")).unwrap_err();
            assert!(
                error.contains(FILE) && error.contains("food.water") && error.contains(message),
                "{error}"
            );
        }
    }
}
