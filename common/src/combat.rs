//! Shared combat rules. Only the server applies these rules to live entities.
use bevy::prelude::*;
use rkyv::{Archive, Deserialize, Serialize};

use crate::{ClientInput, WeaponKind};

mod anatomy;
pub use anatomy::*;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub enum AmmoKind {
    #[default]
    Ball,
    Ap,
}

impl AmmoKind {
    pub fn penetration(self, weapon: WeaponKind) -> f32 {
        let base = weapon.spec().penetration;
        base * if self == Self::Ap { 1.8 } else { 1.0 }
    }

    pub fn damage(self, weapon: WeaponKind) -> f32 {
        weapon.spec().damage * if self == Self::Ap { 0.85 } else { 1.0 }
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
    pub primary: WeaponKind,
    pub ammo: AmmoKind,
    pub armor: ArmorKind,
    pub helmet: bool,
}

impl Default for Loadout {
    fn default() -> Self {
        Self {
            primary: WeaponKind::Rifle,
            ammo: AmmoKind::Ball,
            armor: ArmorKind::Light,
            helmet: true,
        }
    }
}

impl Loadout {
    pub fn validated(mut self) -> Self {
        if self.primary == WeaponKind::Pistol {
            self.primary = WeaponKind::Rifle;
        }
        self
    }

    pub fn allows(self, weapon: WeaponKind) -> bool {
        weapon == self.primary || weapon == WeaponKind::Pistol
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
    pub magazines: [u32; 3],
    pub reserves: [u32; 3],
    pub active: WeaponKind,
    pub reload_remaining: f32,
    pub loadout: Loadout,
    reload_weapon: Option<WeaponKind>,
    last_reload: u32,
    last_trigger: u32,
    last_shot_at: f32,
}

impl Arsenal {
    pub fn new(loadout: Loadout) -> Self {
        let loadout = loadout.validated();
        let magazines = [
            WeaponKind::Rifle.spec().magazine_size,
            WeaponKind::Pistol.spec().magazine_size,
            WeaponKind::Smg.spec().magazine_size,
        ];
        Self {
            magazines,
            reserves: magazines.map(|count| count * 3),
            active: loadout.primary,
            reload_remaining: 0.0,
            loadout,
            reload_weapon: None,
            last_reload: 0,
            last_trigger: 0,
            last_shot_at: f32::NEG_INFINITY,
        }
    }

    /// Returns an authorized shot, never a client-authoritative projectile.
    pub fn step(&mut self, input: &ClientInput, delta: f32, now: f32) -> Option<WeaponKind> {
        let pressed = input.fire_pressed_sequence != self.last_trigger;
        self.last_trigger = input.fire_pressed_sequence;
        if self.loadout.allows(input.weapon) && self.active != input.weapon {
            self.active = input.weapon;
            self.reload_remaining = 0.0;
            self.reload_weapon = None;
        }
        if self.reload_remaining > 0.0 {
            self.reload_remaining = (self.reload_remaining - delta).max(0.0);
            if self.reload_remaining == 0.0 {
                if let Some(weapon) = self.reload_weapon.take() {
                    let i = weapon.index();
                    let rounds =
                        (weapon.spec().magazine_size - self.magazines[i]).min(self.reserves[i]);
                    self.magazines[i] += rounds;
                    self.reserves[i] -= rounds;
                }
            }
        }
        let i = self.active.index();
        let spec = self.active.spec();
        if input.reload_sequence != self.last_reload {
            self.last_reload = input.reload_sequence;
            if self.reload_remaining == 0.0
                && self.magazines[i] < spec.magazine_size
                && self.reserves[i] > 0
            {
                self.reload_remaining = spec.reload_seconds;
                self.reload_weapon = Some(self.active);
            }
        }
        let wants_fire = if spec.automatic { input.fire } else { pressed };
        if !wants_fire
            || self.reload_remaining > 0.0
            || self.magazines[i] == 0
            || now - self.last_shot_at < spec.seconds_per_shot()
        {
            return None;
        }
        self.magazines[i] -= 1;
        self.last_shot_at = now;
        Some(self.active)
    }

    pub fn synchronize_sequences(&mut self, input: &ClientInput) {
        self.last_reload = input.reload_sequence;
        self.last_trigger = input.fire_pressed_sequence;
    }
}

#[cfg(test)]
mod tests;
