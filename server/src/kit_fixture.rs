//! Opt-in initial equipment for native kit acceptance. Never a reconfiguration
//! command: each spawn/raid receives newly owned physical magazines.
use bevy::prelude::*;
use common::{
    EquipmentSlot, WeaponId,
    catalog::{Attachment, DefinitionId, WeaponConfiguration, WeaponKit, catalog},
    combat::{Arsenal, Loadout},
};

#[derive(Default, Resource)]
pub(crate) struct KitFixture(Option<WeaponConfiguration>);

impl KitFixture {
    pub(crate) fn from_environment() -> Self {
        match std::env::var("TK_PLAYTEST_WEAPON_KIT") {
            Ok(name) => {
                let fixture = Self::named(&name);
                info!("INITIAL EQUIPMENT FIXTURE {name}");
                fixture
            }
            Err(std::env::VarError::NotPresent) => Self::default(),
            Err(error) => panic!("Invalid TK_PLAYTEST_WEAPON_KIT: {error}"),
        }
    }

    fn named(name: &str) -> Self {
        assert!(
            matches!(name, "compact" | "suppressed" | "afg"),
            "Unknown weapon kit {name}"
        );
        let mut config = catalog().configuration(WeaponId::M4A1).unwrap();
        for part in &mut config.components {
            match part.node {
                1 if name != "suppressed" => part.definition = DefinitionId(120),
                2 => {
                    part.definition = DefinitionId(122);
                    part.position = if name == "suppressed" { 5 } else { 0 };
                }
                3 if name == "compact" => part.definition = DefinitionId(124),
                4 if name != "suppressed" => part.definition = DefinitionId(126),
                5 if name != "compact" => part.definition = DefinitionId(128),
                _ => {}
            }
        }
        let node = |node, definition, parent, socket: &str, position| Attachment {
            node,
            definition: DefinitionId(definition),
            parent,
            socket: socket.into(),
            position,
        };
        if name == "suppressed" {
            config.components.extend([
                node(8, 129, Some(5), "suppressor", 0),
                node(9, 132, None, "top", 3),
                node(10, 133, Some(9), "optic", 0),
                node(11, 134, Some(3), "bottom", 9),
            ]);
        } else if name == "afg" {
            config.components.push(node(11, 135, Some(3), "bottom", 0));
        }
        catalog()
            .resolve(&config)
            .expect("valid initial kit fixture");
        Self(Some(config))
    }

    pub(crate) fn arsenal(&self, loadout: Loadout) -> Arsenal {
        let primary = self
            .0
            .as_ref()
            .filter(|_| loadout.primary == WeaponId::M4A1)
            .cloned()
            .unwrap_or_else(|| catalog().configuration(loadout.primary).unwrap());
        Arsenal::from_kits(
            catalog(),
            loadout,
            &[
                (
                    EquipmentSlot::Primary,
                    WeaponKit::from_configuration(catalog(), primary).unwrap(),
                ),
                (
                    EquipmentSlot::Sidearm,
                    WeaponKit::from_configuration(
                        catalog(),
                        catalog().configuration(WeaponId::M1911).unwrap(),
                    )
                    .unwrap(),
                ),
            ],
        )
        .expect("valid initial fixture and owned magazines")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::catalog::EquippedWeapon;

    #[test]
    fn kit_fixture_seeds_owned_magazines_and_preserves_other_loadouts() {
        for (name, capacity) in [("compact", 20), ("suppressed", 30), ("afg", 20)] {
            let fixture = KitFixture::named(name);
            let first = fixture.arsenal(Loadout::default());
            let second = fixture.arsenal(Loadout::default());
            for arsenal in [&first, &second] {
                assert_eq!(arsenal.current().rounds(), capacity);
                assert_eq!(arsenal.current().spec().magazine_size, capacity);
                assert!(
                    arsenal
                        .current()
                        .mechanics()
                        .magazines()
                        .iter()
                        .all(|m| m.capacity == capacity)
                );
                EquippedWeapon::restore(catalog(), arsenal.current().instance().clone()).unwrap();
                assert_eq!(
                    arsenal
                        .equipped(EquipmentSlot::Sidearm)
                        .unwrap()
                        .spec()
                        .definition,
                    WeaponId::M1911
                );
            }
            assert_ne!(
                first.current().instance().id,
                second.current().instance().id
            );
            assert_ne!(
                first.current().mechanics().inserted().unwrap().id,
                second.current().mechanics().inserted().unwrap().id
            );
            let smg = fixture.arsenal(Loadout {
                primary: WeaponId::MP5,
                ..default()
            });
            assert_eq!(
                smg.current().instance().configuration,
                catalog().configuration(WeaponId::MP5).unwrap()
            );
        }
        assert_eq!(
            KitFixture::default()
                .arsenal(Loadout::default())
                .current()
                .spec()
                .magazine_size,
            30
        );
    }
}
