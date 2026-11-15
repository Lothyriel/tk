use crate::sync::ClientSession;
use bevy::prelude::*;
use common::{
    EquipmentSlot, WeaponId,
    catalog::{Attachment, DefinitionId, WeaponKit, catalog},
    replication::{CommandOutcome, ConfigurationCommand},
};

/// UI locations for the first slice. These describe controls, never fit rules:
/// every draft and every server transaction goes through the shared resolver.
pub(super) struct Row {
    pub name: &'static str,
    pub node: u16,
    pub parent: Option<u16>,
    pub socket: &'static str,
    pub choices: &'static [u32],
}

pub(super) const ROWS: &[Row] = &[
    Row {
        name: "Magazine",
        node: 1,
        parent: None,
        socket: "magazine",
        choices: &[101, 120],
    },
    Row {
        name: "Stock",
        node: 2,
        parent: None,
        socket: "stock",
        choices: &[121, 122],
    },
    Row {
        name: "Handguard",
        node: 3,
        parent: None,
        socket: "handguard",
        choices: &[123, 124],
    },
    Row {
        name: "Barrel",
        node: 4,
        parent: None,
        socket: "barrel",
        choices: &[125, 126],
    },
    Row {
        name: "Muzzle",
        node: 5,
        parent: Some(4),
        socket: "muzzle",
        choices: &[127, 128],
    },
    Row {
        name: "Front iron",
        node: 6,
        parent: Some(4),
        socket: "front_sight",
        choices: &[130],
    },
    Row {
        name: "Rear iron",
        node: 7,
        parent: None,
        socket: "top",
        choices: &[0, 131],
    },
    Row {
        name: "Suppressor",
        node: 8,
        parent: Some(5),
        socket: "suppressor",
        choices: &[0, 129],
    },
    Row {
        name: "Optic mount",
        node: 9,
        parent: None,
        socket: "top",
        choices: &[0, 132],
    },
    Row {
        name: "Optic",
        node: 10,
        parent: Some(9),
        socket: "optic",
        choices: &[0, 133],
    },
    Row {
        name: "Grip",
        node: 11,
        parent: Some(3),
        socket: "bottom",
        choices: &[0, 134, 135],
    },
];

#[derive(Resource, Default)]
pub(crate) struct Editor {
    pub visible: bool,
    pub slot: EquipmentSlot,
    pub draft: Option<WeaponKit>,
    pub status: String,
    pub pending: Option<u64>,
    epoch: u64,
    next_request: u64,
    source: Option<(u64, u32)>,
}

impl Editor {
    pub fn sync(&mut self, session: &ClientSession, id: u64) {
        let cache = &session.configurations.cache;
        if cache.epoch() != self.epoch {
            self.epoch = cache.epoch();
            self.next_request = 0;
            self.pending = None;
            self.source = None;
            self.draft = None;
            self.status.clear();
        }
        if !matches!(
            session.raid.phase,
            common::raid::RaidPhase::Lobby | common::raid::RaidPhase::Countdown
        ) {
            self.visible = false;
        }
        let Some(entry) = cache.latest(id, self.slot) else {
            return;
        };
        if let Some(request) = self.pending {
            let Some(result) = session
                .configurations
                .last_result
                .as_ref()
                .filter(|result| result.request == request && result.epoch == self.epoch)
            else {
                return;
            };
            if result.outcome != CommandOutcome::Applied {
                self.pending = None;
                self.status = result
                    .reason
                    .clone()
                    .unwrap_or_else(|| format!("{:?}", result.outcome));
                return;
            }
            if result.revision != Some(entry.packet.revision) {
                return; // Result can arrive before publication.
            }
            self.pending = None;
            self.source = None;
            self.status = "Server confirmed the kit.".into();
        }
        let source = (entry.packet.epoch, entry.packet.handle);
        if self.source != Some(source) {
            self.draft = entry.packet.kit(catalog()).ok();
            self.source = Some(source);
        }
    }

    pub fn editable(&self, session: &ClientSession, id: u64) -> bool {
        self.pending.is_none()
            && session.raid.phase == common::raid::RaidPhase::Lobby
            && session.local(id).is_some_and(|p| !p.participant.ready)
    }

    pub fn switch_slot(&mut self) {
        if self.pending.is_some() {
            return;
        }
        self.slot = if self.slot == EquipmentSlot::Primary {
            EquipmentSlot::Sidearm
        } else {
            EquipmentSlot::Primary
        };
        self.source = None;
        self.draft = None;
        self.status.clear();
    }

    pub fn reset(&mut self) {
        if self.pending.is_none() {
            self.source = None;
            self.status.clear();
        }
    }

    pub fn cycle_part(&mut self, row: usize) {
        let Some(kit) = &mut self.draft else { return };
        let row = &ROWS[row];
        if kit.weapon.definition != WeaponId::M4A1 {
            return;
        }
        let current = kit.weapon.components.iter().find(|p| p.node == row.node);
        let definition = current.map_or(0, |p| p.definition.0);
        let position = current.map_or(if row.node == 7 { 12 } else { 0 }, |p| p.position);
        let next = row.choices[(row
            .choices
            .iter()
            .position(|id| *id == definition)
            .unwrap_or(0)
            + 1)
            % row.choices.len()];
        if next == 0 {
            // Explicit subtree removal in the draft; publication is one atomic
            // whole-kit command, never a sequence of hidden dependent edits.
            let mut removed = vec![row.node];
            loop {
                let old_len = removed.len();
                for part in &kit.weapon.components {
                    if part.parent.is_some_and(|p| removed.contains(&p))
                        && !removed.contains(&part.node)
                    {
                        removed.push(part.node);
                    }
                }
                if removed.len() == old_len {
                    break;
                }
            }
            kit.weapon.components.retain(|p| !removed.contains(&p.node));
            self.status = format!(
                "Draft removes {} part(s), including dependents.",
                removed.len()
            );
        } else if let Some(part) = kit
            .weapon
            .components
            .iter_mut()
            .find(|p| p.node == row.node)
        {
            part.definition = DefinitionId(next);
        } else {
            kit.weapon.components.push(Attachment {
                node: row.node,
                definition: DefinitionId(next),
                parent: row.parent,
                socket: row.socket.into(),
                position,
            });
        }
    }

    pub fn cycle_position(&mut self, row: usize) {
        let Some(kit) = &mut self.draft else { return };
        let Some(index) = kit
            .weapon
            .components
            .iter()
            .position(|p| p.node == ROWS[row].node)
        else {
            return;
        };
        let part = &kit.weapon.components[index];
        let geometry = match part.parent {
            Some(parent) => kit
                .weapon
                .components
                .iter()
                .find(|p| p.node == parent)
                .and_then(|p| catalog().component(p.definition))
                .map(|p| &p.assembly.geometry),
            None => catalog()
                .definition(kit.weapon.definition)
                .map(|w| &w.assembly.geometry),
        };
        let positions = geometry
            .and_then(|g| g.sockets.iter().find(|s| s.key == part.socket))
            .map_or(1, |s| s.positions);
        kit.weapon.components[index].position = (part.position + 1) % positions;
    }

    pub fn cycle_reserve(&mut self, index: usize) {
        let Some(kit) = &mut self.draft else { return };
        let options: &[u32] = match kit.weapon.definition {
            WeaponId::M4A1 => &[0, 101, 120],
            WeaponId::M1911 => &[0, 102],
            _ => &[0, 103],
        };
        let mut slots = [0; 3];
        for (slot, definition) in slots.iter_mut().zip(&kit.reserves) {
            *slot = definition.0;
        }
        let current = options
            .iter()
            .position(|id| *id == slots[index])
            .unwrap_or(0);
        slots[index] = options[(current + 1) % options.len()];
        kit.reserves = slots
            .into_iter()
            .filter(|id| *id != 0)
            .map(DefinitionId)
            .collect();
    }

    pub fn command(&mut self, session: &ClientSession, id: u64) -> Option<ConfigurationCommand> {
        if !self.editable(session, id) {
            return None;
        }
        let kit = self.draft.as_ref()?;
        if let Err(error) = kit.resolve(catalog()) {
            self.status = error.to_string();
            return None;
        }
        let entry = session.configurations.cache.latest(id, self.slot)?;
        self.next_request = self.next_request.checked_add(1)?;
        self.pending = Some(self.next_request);
        self.status = "Waiting for server confirmation...".into();
        Some(ConfigurationCommand {
            epoch: self.epoch,
            request: self.next_request,
            slot: self.slot,
            instance: entry.packet.instance,
            expected_revision: entry.packet.revision,
            weapon: kit.weapon.clone(),
            reserves: Some(kit.reserves.clone()),
        })
    }
}

pub(super) fn part_name(definition: u32) -> String {
    if definition == 0 {
        return "None".into();
    }
    catalog().component(DefinitionId(definition)).map_or_else(
        || "Unknown".into(),
        |part| part.key.replace("m4a1_", "").replace('_', " "),
    )
}
