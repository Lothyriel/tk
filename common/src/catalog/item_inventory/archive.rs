//! Portable S1 archive fixture, not a raid snapshot or persistent stash.
use super::*;
use crate::{catalog::item_manifest, data};
use rkyv::{Archive, Deserialize, Serialize};

const LEGACY_SCHEMA: u16 = 1;
const SCHEMA_VERSION: u16 = 2;
const MAGIC: &[u8] = b"\xffTKINV\x00";
const MAX_BYTES: usize = 1024 * 1024;

#[derive(Archive, Serialize, Deserialize)]
struct ContentDocument {
    layout: ArchiveDocument,
    // Same order as layout.items. Never sort them independently.
    contents: Vec<ItemContents>,
}

#[derive(Archive, Serialize, Deserialize)]
struct ArchiveDocument {
    schema_version: u16,
    catalog_revision: String,
    last_id: u64,
    compartments: Vec<ArchiveCompartment>,
    items: Vec<ArchiveItem>,
}

#[derive(Archive, Serialize, Deserialize)]
struct ArchiveCompartment {
    id: CompartmentId,
    size: [u8; 2],
    profile: Option<String>,
}

#[derive(Archive, Serialize, Deserialize)]
struct ArchiveItem {
    id: ItemInstanceId,
    definition: String,
    condition: Condition,
    quantity: u16,
    location: GridLocation,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ArchiveError {
    TooLarge,
    Malformed,
    UnsupportedSchema(u16),
    UnknownRevision(String),
    InvalidLayout(CompartmentId),
    Compartment(CompartmentId, InventoryError),
    InvalidId(ItemInstanceId),
    InvalidItem(ItemInstanceId, InventoryError),
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use ArchiveError::*;
        write!(f, "inventory archive: ")?;
        match self {
            TooLarge => write!(f, "size: exceeds {MAX_BYTES} bytes"),
            Malformed => write!(f, "encoding: malformed data"),
            UnsupportedSchema(v) => write!(f, "schema_version: unsupported {v}"),
            UnknownRevision(revision) => write!(f, "catalog_revision: unknown {revision:?}"),
            InvalidLayout(id) => write!(f, "compartment {}: size: invalid layout", id.0),
            Compartment(id, error) => write!(f, "compartment {}: id: {error:?}", id.0),
            InvalidId(id) => write!(f, "item {}: id: invalid or duplicate", id.0),
            InvalidItem(id, error) => {
                let field = match error {
                    InventoryError::UnknownDefinition => "definition",
                    InventoryError::InvalidQuantity => "quantity",
                    InventoryError::InvalidContents => "contents",
                    _ => "location",
                };
                write!(f, "item {}: {field}: {error:?}", id.0)
            }
        }
    }
}

impl std::error::Error for ArchiveError {}

impl Inventory {
    pub fn export_archive(&self) -> Result<bytes::Bytes, ArchiveError> {
        let layout = ArchiveDocument {
            schema_version: SCHEMA_VERSION,
            catalog_revision: item_manifest::REVISION.into(),
            last_id: self.last_id,
            compartments: self
                .compartments
                .iter()
                .map(|(&id, c)| ArchiveCompartment {
                    id,
                    size: c.size.dimensions(),
                    profile: c.profile.clone(),
                })
                .collect(),
            items: self
                .items
                .iter()
                .map(|(&id, i)| ArchiveItem {
                    id,
                    definition: i.definition.id.into(),
                    condition: i.condition,
                    quantity: i.quantity,
                    location: i.location,
                })
                .collect(),
        };
        let bytes = data::encode(&ContentDocument {
            layout,
            contents: self.items.values().map(|item| item.contents).collect(),
        });
        if bytes.len() > MAX_BYTES - MAGIC.len() {
            return Err(ArchiveError::TooLarge);
        }
        let mut framed = Vec::with_capacity(MAGIC.len() + bytes.len());
        framed.extend_from_slice(MAGIC);
        framed.extend_from_slice(&bytes);
        Ok(framed.into())
    }

    /// Validate the whole archive in a detached collection before returning it.
    pub fn import_archive(bytes: &[u8]) -> Result<Self, ArchiveError> {
        if bytes.len() > MAX_BYTES {
            return Err(ArchiveError::TooLarge);
        }
        let (doc, contents) = if let Some(payload) = bytes.strip_prefix(MAGIC) {
            let doc = data::try_decode::<ContentDocument>(payload)
                .map_err(|_| ArchiveError::Malformed)?;
            if doc.layout.schema_version != SCHEMA_VERSION {
                return Err(ArchiveError::UnsupportedSchema(doc.layout.schema_version));
            }
            if doc.contents.len() != doc.layout.items.len() {
                return Err(ArchiveError::Malformed);
            }
            (doc.layout, Some(doc.contents))
        } else {
            let doc =
                data::try_decode::<ArchiveDocument>(bytes).map_err(|_| ArchiveError::Malformed)?;
            if !matches!(doc.schema_version, 0 | LEGACY_SCHEMA) {
                return Err(ArchiveError::UnsupportedSchema(doc.schema_version));
            }
            (doc, None)
        };
        if doc.catalog_revision != item_manifest::REVISION {
            return Err(ArchiveError::UnknownRevision(doc.catalog_revision));
        }
        let mut result = Inventory::default();
        for c in doc.compartments {
            let [w, h] = c.size;
            let size = Footprint::new(w, h).ok_or(ArchiveError::InvalidLayout(c.id))?;
            result
                .register_compartment(c.id, size, c.profile)
                .map_err(|err| ArchiveError::Compartment(c.id, err))?;
        }
        for (index, i) in doc.items.into_iter().enumerate() {
            if i.id.0 == 0 || i.id.0 > doc.last_id || result.items.contains_key(&i.id) {
                return Err(ArchiveError::InvalidId(i.id));
            }
            // The v0 fixture uses the same fields and a pre-namespace ID.
            // v1 is canonical-only; unknown references never become defaults.
            let definition = match (doc.schema_version, i.definition.as_str()) {
                (0, "bandage") => "med.bandage",
                (_, id) => id,
            };
            let invalid = |err| ArchiveError::InvalidItem(i.id, err);
            let mut item = ItemInstance::seed(definition, i.condition, i.quantity, i.location)
                .map_err(invalid)?;
            if let Some(contents) = &contents {
                item = item.with_contents(contents[index]).map_err(invalid)?;
            }
            result.validate_placement(item, None).map_err(invalid)?;
            result.items.insert(i.id, item);
        }
        result.last_id = doc.last_id;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::item_grid::Rotation;

    fn fixture() -> ArchiveDocument {
        ArchiveDocument {
            schema_version: 0,
            catalog_revision: item_manifest::REVISION.into(),
            last_id: 8,
            compartments: vec![ArchiveCompartment {
                id: CompartmentId(1),
                size: [4, 4],
                profile: Some("med_bleed".into()),
            }],
            items: vec![ArchiveItem {
                id: ItemInstanceId(4),
                definition: "bandage".into(),
                condition: Condition::Damaged,
                quantity: 2,
                location: GridLocation {
                    compartment: CompartmentId(1),
                    placement: Placement {
                        x: 1,
                        y: 2,
                        rotation: Rotation::QuarterTurn,
                    },
                },
            }],
        }
    }

    #[test]
    fn item_archive_migrates_and_preserves_state_and_retired_id_boundary() {
        let fixture = fixture();
        let bytes = data::encode(&fixture);
        let mut unaligned = rkyv::util::AlignedVec::<16>::new();
        unaligned.push(0);
        unaligned.extend_from_slice(&bytes);
        let mut inventory = Inventory::import_archive(&unaligned[1..]).unwrap();
        let exported = inventory.export_archive().unwrap();
        let migrated =
            data::try_decode::<ContentDocument>(exported.strip_prefix(MAGIC).unwrap()).unwrap();
        assert_eq!(migrated.layout.schema_version, SCHEMA_VERSION);
        let item = inventory.get(ItemInstanceId(4)).unwrap();
        assert_eq!(item.definition.id, "med.bandage");
        assert_eq!(item.condition, fixture.items[0].condition);
        assert_eq!(item.quantity, fixture.items[0].quantity);
        assert_eq!(item.location, fixture.items[0].location);
        assert_eq!(
            Inventory::import_archive(&exported)
                .unwrap()
                .export_archive()
                .unwrap(),
            exported
        );
        let mut location = item.location;
        location.placement.x = 0;
        assert_eq!(
            inventory.split(ItemInstanceId(4), 1, location),
            Ok(ItemInstanceId(9))
        );
    }

    #[test]
    fn item_archive_roundtrips_contents_and_rejects_incompatible_payload() {
        let mut inventory = Inventory::default();
        let compartment = CompartmentId(1);
        inventory
            .register_compartment(compartment, Footprint::new(4, 4).unwrap(), None)
            .unwrap();
        let location = GridLocation {
            compartment,
            placement: Placement {
                x: 0,
                y: 0,
                rotation: Rotation::Upright,
            },
        };
        let water = ItemContents::Water { millilitres: 175 };
        let id = inventory
            .insert_seed_with_contents("food.water500", Condition::Intact, 1, location, water)
            .unwrap();
        let exported = inventory.export_archive().unwrap();
        let restored = Inventory::import_archive(&exported).unwrap();
        assert_eq!(restored.get(id), inventory.get(id));
        assert_eq!(restored.get(id).unwrap().mass_g(), 205);
        let mut doc =
            data::try_decode::<ContentDocument>(exported.strip_prefix(MAGIC).unwrap()).unwrap();
        doc.contents[0] = ItemContents::EmptyMagazine;
        let mut invalid = MAGIC.to_vec();
        invalid.extend_from_slice(&data::encode(&doc));
        assert!(matches!(
            Inventory::import_archive(&invalid),
            Err(ArchiveError::InvalidItem(bad, InventoryError::InvalidContents)) if bad == id
        ));
    }

    #[test]
    fn item_archive_rejects_invalid_wire_versions_references_and_geometry() {
        let mut doc = fixture();
        assert!(matches!(
            Inventory::import_archive(&[]),
            Err(ArchiveError::Malformed)
        ));
        doc.schema_version = 99;
        assert!(matches!(
            Inventory::import_archive(&data::encode(&doc)),
            Err(ArchiveError::UnsupportedSchema(99))
        ));
        doc.schema_version = LEGACY_SCHEMA;
        assert!(matches!(
            Inventory::import_archive(&data::encode(&doc)),
            Err(ArchiveError::InvalidItem(
                ItemInstanceId(4),
                InventoryError::UnknownDefinition
            ))
        ));
        doc.items[0].definition = "med.bandage".into();
        doc.items[0].location.placement.x = 4;
        assert!(matches!(
            Inventory::import_archive(&data::encode(&doc)),
            Err(ArchiveError::InvalidItem(
                ItemInstanceId(4),
                InventoryError::Placement(PlacementError::OutsideGrid)
            ))
        ));
    }
}
