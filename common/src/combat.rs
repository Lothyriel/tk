//! Shared combat rules. Only the server applies these rules to live entities.
use bevy::prelude::*;
use rkyv::{Archive, Deserialize, Serialize};

use crate::{
    catalog::{catalog, Catalog, CatalogError, EquippedWeapon, ResolvedWeapon},
    ClientInput, EquipmentSlot, WeaponId, WeaponInstanceId, WeaponKind,
};

mod anatomy;
pub use anatomy::*;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub enum AmmoKind {
    #[default]
    Ball,
    Ap,
}

impl AmmoKind {
    pub fn penetration(self, weapon: &ResolvedWeapon) -> f32 {
        let base = weapon.penetration;
        base * if self == Self::Ap { 1.8 } else { 1.0 }
    }

    pub fn damage(self, weapon: &ResolvedWeapon) -> f32 {
        weapon.damage * if self == Self::Ap { 0.85 } else { 1.0 }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub enum ArmorKind {
    None,
    #[default]
    Light,
    Heavy,
}

impl ArmorKind {
    pub fn protection(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Light => 18.0,
            Self::Heavy => 40.0,
        }
    }

    pub fn durability(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Light => 60.0,
            Self::Heavy => 100.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct Loadout {
    pub primary: WeaponId,
    pub ammo: AmmoKind,
    pub armor: ArmorKind,
    pub helmet: bool,
}

impl Default for Loadout {
    fn default() -> Self {
        Self {
            primary: WeaponId::M4A1,
            ammo: AmmoKind::Ball,
            armor: ArmorKind::Light,
            helmet: true,
        }
    }
}

impl Loadout {
    pub fn validated(mut self) -> Option<Self> {
        if catalog().weapon(self.primary)?.class == WeaponKind::Pistol {
            self.primary = WeaponId::M4A1;
        }
        Some(self)
    }

    pub fn allows(self, weapon: WeaponId) -> bool {
        weapon == self.primary || weapon == WeaponId::M1911
    }

    pub fn weapon(self, slot: EquipmentSlot) -> WeaponId {
        match slot {
            EquipmentSlot::Primary => self.primary,
            EquipmentSlot::Sidearm => WeaponId::M1911,
        }
    }
}

#[derive(Debug, Default, Clone, Component, Archive, Serialize, Deserialize)]
pub struct ArmorState {
    pub kind: ArmorKind,
    pub vest: f32,
    pub helmet: f32,
}

impl ArmorState {
    pub fn from_loadout(loadout: Loadout) -> Self {
        Self {
            kind: loadout.armor,
            vest: loadout.armor.durability(),
            helmet: if loadout.helmet { 40.0 } else { 0.0 },
        }
    }

    fn absorb(&mut self, part: BodyPart, damage: f32, penetration: f32) -> f32 {
        let (durability, max_durability, protection) = match part {
            BodyPart::Head => (&mut self.helmet, 40.0, 22.0),
            BodyPart::Thorax => (
                &mut self.vest,
                self.kind.durability(),
                self.kind.protection(),
            ),
            _ => return damage,
        };
        if *durability <= 0.0 || max_durability <= 0.0 {
            return damage;
        }
        let stopped = penetration < protection * (*durability / max_durability);
        *durability = (*durability - damage * 0.4).max(0.0);
        damage * if stopped { 0.2 } else { 0.8 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
#[repr(usize)]
pub enum BodyPart {
    Head,
    Thorax,
    Arms,
    Legs,
}

impl BodyPart {
    /// Coordinates relative to the standing capsule center; crouch is normalized by the caller.
    pub fn at_local_point(point: Vec3) -> Self {
        if point.y > 0.45 {
            Self::Head
        } else if point.y < -0.25 {
            Self::Legs
        } else if point.x.abs() > 0.23 {
            Self::Arms
        } else {
            Self::Thorax
        }
    }
}

#[derive(Debug, Clone, Component, Archive, Serialize, Deserialize)]
pub struct BodyHealth {
    pub parts: [f32; 4],
    pub last_hit: Option<BodyPart>,
    pub anatomy: Anatomy,
}

impl Default for BodyHealth {
    fn default() -> Self {
        Self {
            parts: [35.0, 85.0, 60.0, 70.0],
            last_hit: None,
            anatomy: Anatomy::default(),
        }
    }
}

impl BodyHealth {
    pub fn alive(&self) -> bool {
        self.parts[0] > 0.0 && self.parts[1] > 0.0 && self.anatomy.alive()
    }

    pub fn total(&self) -> f32 {
        self.parts.iter().sum()
    }

    pub fn speed_factor(&self) -> f32 {
        let legacy: f32 = if self.parts[3] < 35.0 { 0.55 } else { 1.0 };
        legacy.min(self.anatomy.speed())
    }

    pub fn hit(&mut self, part: BodyPart, damage: f32, penetration: f32, armor: &mut ArmorState) {
        if !self.alive() || !damage.is_finite() || damage <= 0.0 {
            return;
        }
        let damage = armor.absorb(part, damage, penetration);
        let health = &mut self.parts[part as usize];
        let overflow = (damage - *health).max(0.0);
        *health = (*health - damage).max(0.0);
        if matches!(part, BodyPart::Arms | BodyPart::Legs) {
            self.parts[1] = (self.parts[1] - overflow * 0.5).max(0.0);
        }
        self.last_hit = Some(part);
    }
}

#[derive(Debug, Clone, Copy, Component)]
pub enum SurfaceKind {
    Wood,
    Concrete,
}

impl SurfaceKind {
    pub fn penetrate(self, energy: f32, thickness: f32) -> Option<f32> {
        let remaining = match self {
            Self::Wood => energy - 20.0 * thickness.max(0.0),
            Self::Concrete => return None,
        };
        (remaining > 0.0).then_some(remaining)
    }
}

#[derive(Debug, Component)]
pub struct Arsenal {
    pub recoil: crate::recoil::RecoilState,
    equipment: Vec<(EquipmentSlot, EquippedWeapon)>,
    kits: Vec<(EquipmentSlot, crate::catalog::WeaponKit)>,
    pub active: EquipmentSlot,
    /// Derived compatibility value for HUD; never drives mechanical commits.
    pub reload_remaining: f32,
    pub loadout: Loadout,
    pending_slot: Option<EquipmentSlot>,
    last_reload: u32,
    last_trigger: u32,
    last_inspect: u32,
    last_selector: u32,
    last_shot_at: f32,
}

impl Arsenal {
    pub fn new(loadout: Loadout) -> Self {
        let loadout = loadout.validated().expect("validated loadout");
        Self::from_slots(
            catalog(),
            loadout,
            &[
                (EquipmentSlot::Primary, loadout.primary),
                (EquipmentSlot::Sidearm, WeaponId::M1911),
            ],
        )
        .expect("valid equipment")
    }

    /// Slots own instances, never an array indexed by the number of definitions.
    pub fn from_slots(
        catalog: &Catalog,
        loadout: Loadout,
        slots: &[(EquipmentSlot, WeaponId)],
    ) -> Result<Self, CatalogError> {
        let kits = slots
            .iter()
            .map(|&(slot, definition)| {
                crate::catalog::WeaponKit::from_configuration(
                    catalog,
                    catalog.configuration(definition)?,
                )
                .map(|kit| (slot, kit))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::from_kits(catalog, loadout, &kits)
    }

    /// Materialize a new raid's equipment. Selections may survive a raid;
    /// physical weapon/magazine identities and spent ammunition do not.
    pub fn from_kits(
        catalog: &Catalog,
        mut loadout: Loadout,
        kits: &[(EquipmentSlot, crate::catalog::WeaponKit)],
    ) -> Result<Self, CatalogError> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);
        let mut equipment = Vec::new();
        for (slot, kit) in kits {
            let slot = *slot;
            if equipment.iter().any(|(existing, _)| *existing == slot) {
                return Err(CatalogError {
                    file: crate::catalog::CATALOG_FILE.into(),
                    field: "equipment.slot".into(),
                    message: "duplicate equipment slot".into(),
                });
            }
            let id = NEXT_INSTANCE
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                    next.checked_add(1)
                })
                .map(WeaponInstanceId)
                .map_err(|_| CatalogError {
                    file: crate::catalog::CATALOG_FILE.into(),
                    field: "equipment.instance".into(),
                    message: "identity exhausted".into(),
                })?;
            let mut weapon = EquippedWeapon::from_kit(catalog, id, kit)?;
            if slot != EquipmentSlot::Primary {
                weapon.mechanics_mut().start_unequip(0.01);
                weapon.advance(0.01);
            }
            equipment.push((slot, weapon));
        }
        let Some((_, primary)) = equipment
            .iter()
            .find(|(slot, _)| *slot == EquipmentSlot::Primary)
        else {
            return Err(CatalogError {
                file: crate::catalog::CATALOG_FILE.into(),
                field: "equipment.primary".into(),
                message: "missing primary slot".into(),
            });
        };
        loadout.primary = primary.spec().definition;
        Ok(Self {
            recoil: Default::default(),
            equipment,
            kits: kits.to_vec(),
            active: EquipmentSlot::Primary,
            reload_remaining: 0.0,
            loadout,
            pending_slot: None,
            last_reload: 0,
            last_trigger: 0,
            last_inspect: 0,
            last_selector: 0,
            last_shot_at: f32::NEG_INFINITY,
        })
    }

    pub fn equipped(&self, slot: EquipmentSlot) -> Option<&EquippedWeapon> {
        self.equipment
            .iter()
            .find(|(s, _)| *s == slot)
            .map(|(_, weapon)| weapon)
    }

    pub fn equipped_mut(&mut self, slot: EquipmentSlot) -> Option<&mut EquippedWeapon> {
        self.equipment
            .iter_mut()
            .find(|(s, _)| *s == slot)
            .map(|(_, weapon)| weapon)
    }

    pub fn current(&self) -> &EquippedWeapon {
        self.equipped(self.active).expect("equipped active slot")
    }

    pub fn kits(&self) -> &[(EquipmentSlot, crate::catalog::WeaponKit)] {
        &self.kits
    }

    /// Reissue the confirmed selections, not the current magazine/reload state.
    pub fn restocked(&self, catalog: &Catalog, loadout: Loadout) -> Result<Self, CatalogError> {
        Self::from_kits(catalog, loadout, &self.kits)
    }

    /// Free equipment selection. The caller must enforce the unready lobby
    /// policy; physical reconfiguration below remains ammunition-conservative.
    pub fn select_kit(
        &mut self,
        catalog: &Catalog,
        slot: EquipmentSlot,
        expected_revision: u64,
        mut kit: crate::catalog::WeaponKit,
    ) -> Result<u64, CatalogError> {
        let fail = |message: &str| CatalogError {
            file: crate::catalog::CATALOG_FILE.into(),
            field: "equipment.kit".into(),
            message: message.into(),
        };
        if self.current().mechanics().action().is_some() {
            return Err(fail("cannot select equipment during manipulation"));
        }
        let old = self.equipped(slot).ok_or_else(|| fail("empty slot"))?;
        if old.instance().revision != expected_revision {
            return Err(fail("stale configuration revision"));
        }
        if old.spec().definition != kit.weapon.definition {
            return Err(fail("cannot replace instance identity"));
        }
        kit.weapon.components.sort_by_key(|part| part.node);
        let index = self.kits.iter().position(|(s, _)| *s == slot).unwrap();
        if self.kits[index].1 == kit {
            return Ok(expected_revision);
        }
        let revision = expected_revision
            .checked_add(1)
            .ok_or_else(|| fail("revision exhausted"))?;
        let replacement = EquippedWeapon::from_kit(catalog, old.instance().id, &kit)?;
        let mut instance = replacement.instance().clone();
        instance.revision = revision;
        let mut replacement = EquippedWeapon::restore(catalog, instance)?;
        if self.active != slot {
            replacement.mechanics_mut().start_unequip(0.01);
            replacement.advance(0.01);
        }
        *self.equipped_mut(slot).unwrap() = replacement;
        self.kits[index].1 = kit;
        Ok(revision)
    }

    /// Authoritative configuration boundary. Lobby ownership/ready locks are
    /// enforced by the caller; this layer rejects stale or invalid assemblies.
    pub fn reconfigure(
        &mut self,
        catalog: &Catalog,
        slot: EquipmentSlot,
        expected_revision: u64,
        configuration: crate::catalog::WeaponConfiguration,
    ) -> Result<u64, CatalogError> {
        if self.current().mechanics().action().is_some() {
            return Err(CatalogError {
                file: crate::catalog::CATALOG_FILE.into(),
                field: "equipment.reload".into(),
                message: "cannot reconfigure during reload/manipulation".into(),
            });
        }
        let weapon = self.equipped_mut(slot).ok_or_else(|| CatalogError {
            file: crate::catalog::CATALOG_FILE.into(),
            field: "equipment.slot".into(),
            message: "empty slot".into(),
        })?;
        let revision = weapon.reconfigure(catalog, expected_revision, configuration)?;
        let kit = weapon.kit();
        // Preserve the multiset of owned magazines when an ordinary edit selects
        // another inserted definition; it must not duplicate that new reserve.
        self.kits.iter_mut().find(|(s, _)| *s == slot).unwrap().1 = kit;
        Ok(revision)
    }

    /// Returns an authorized shot, never a client-authoritative projectile.
    pub fn step(&mut self, input: &ClientInput, delta: f32, now: f32) -> Option<WeaponId> {
        let pressed = fresh_sequence(&mut self.last_trigger, input.fire_pressed_sequence);
        let reload = fresh_sequence(&mut self.last_reload, input.reload_sequence);
        let inspect = fresh_sequence(&mut self.last_inspect, input.inspect_sequence);
        let selector = fresh_sequence(&mut self.last_selector, input.selector_sequence);
        // Intent cancels inspect even when the requested shot/reload cannot
        // complete (safe, cooldown, full magazine). Consume blocked inspect
        // sequences so releasing ADS/sprint never starts a delayed inspection.
        let interrupts_inspect = input.aim
            || input.run
            || input.fire
            || pressed
            || reload
            || input.weapon != self.active;
        if interrupts_inspect
            && self
                .current()
                .mechanics()
                .action()
                .is_some_and(|action| action.kind == crate::catalog::ManipulationKind::Inspect)
        {
            self.equipped_mut(self.active)
                .unwrap()
                .mechanics_mut()
                .cancel();
        }
        if self.pending_slot.is_none()
            && self.equipped(input.weapon).is_some()
            && self.active != input.weapon
            && self
                .equipped_mut(self.active)
                .unwrap()
                .mechanics_mut()
                .start_unequip(crate::catalog::HandlingTiming::UNEQUIP)
        {
            self.pending_slot = Some(input.weapon);
        }
        self.equipped_mut(self.active).unwrap().advance(delta);
        if !self.current().mechanics().equipped() && self.current().mechanics().action().is_none() {
            if let Some(slot) = self.pending_slot.take() {
                self.active = slot;
            }
            self.equipped_mut(self.active)
                .unwrap()
                .mechanics_mut()
                .start_equip(crate::catalog::HandlingTiming::EQUIP);
        }
        if self.pending_slot.is_none() {
            let weapon = self.equipped_mut(self.active).unwrap();
            let duration = weapon.spec().reload_seconds;
            if selector {
                weapon.mechanics_mut().cycle_selector();
            }
            if reload {
                weapon.mechanics_mut().start_reload(duration);
            }
            if inspect && !interrupts_inspect {
                weapon
                    .mechanics_mut()
                    .start_inspect(crate::catalog::HandlingTiming::INSPECT);
            }
        }
        let snapshot = self.current().mechanics().snapshot();
        self.reload_remaining = if snapshot.reloading() {
            snapshot.remaining
        } else {
            0.0
        };
        let definition = self.current().spec().definition;
        if self.pending_slot.is_some()
            || now - self.last_shot_at < self.current().spec().seconds_per_shot()
        {
            return None;
        }
        if !self
            .equipped_mut(self.active)
            .unwrap()
            .mechanics_mut()
            .fire(input.fire, pressed)
        {
            return None;
        }
        self.last_shot_at = now;
        Some(definition)
    }

    pub fn cancel_actions(&mut self) {
        for (_, weapon) in &mut self.equipment {
            weapon.mechanics_mut().cancel();
        }
        self.pending_slot = None;
        self.reload_remaining = 0.0;
    }

    pub fn synchronize_sequences(&mut self, input: &ClientInput) {
        self.last_reload = input.reload_sequence;
        self.last_trigger = input.fire_pressed_sequence;
        self.last_inspect = input.inspect_sequence;
        self.last_selector = input.selector_sequence;
    }
}

fn fresh_sequence(last: &mut u32, incoming: u32) -> bool {
    let distance = incoming.wrapping_sub(*last);
    if distance != 0 && distance < (1 << 31) {
        *last = incoming;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests;
