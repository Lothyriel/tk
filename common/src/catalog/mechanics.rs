//! Persistent closed-bolt state and interruptible manipulation commits.
//! The inventory adapter will move these same magazine IDs, never copy rounds.
use super::*;

pub const MAX_RETAINED_MAGAZINES: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct MagazineInstance {
    pub id: MagazineInstanceId,
    pub definition: DefinitionId,
    pub ammo: DefinitionId,
    pub capacity: u32,
    pub rounds: u32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub enum FireMode {
    Safe,
    Semi,
    #[default]
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub enum ManipulationKind {
    TacticalReload,
    EmptyReload,
    Equip,
    Unequip,
    Inspect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub enum ManipulationPhase {
    Remove,
    Insert,
    Feed,
    Finish,
    Equip,
    Unequip,
    Inspect,
}

#[derive(Debug, Clone, PartialEq, Archive, Serialize, Deserialize)]
pub struct Manipulation {
    pub id: u64,
    pub kind: ManipulationKind,
    pub phase: ManipulationPhase,
    /// Mechanical time, advanced by authoritative delta * handling rate.
    pub elapsed: f32,
    pub duration: f32,
    pub target_magazine: Option<MagazineInstanceId>,
}

impl Manipulation {
    /// Stable normalized interval for presentation; phase is explicit even when
    /// rounds/chamber happen to be identical on either side of a commit.
    pub fn fraction(&self) -> f32 {
        let (start, end) = self.interval();
        start + (end - start) * (self.elapsed / self.phase_duration()).clamp(0.0, 1.0)
    }

    pub fn remaining(&self) -> f32 {
        self.duration * (1.0 - self.fraction())
    }

    fn interval(&self) -> (f32, f32) {
        match self.phase {
            ManipulationPhase::Remove => (0.0, 0.25),
            ManipulationPhase::Insert => (0.25, 0.70),
            ManipulationPhase::Feed => (0.70, 0.90),
            ManipulationPhase::Finish => (
                if self.kind == ManipulationKind::EmptyReload {
                    0.90
                } else {
                    0.70
                },
                1.0,
            ),
            _ => (0.0, 1.0),
        }
    }

    fn phase_duration(&self) -> f32 {
        let (start, end) = self.interval();
        self.duration * (end - start)
    }
}

#[derive(Debug, Clone, PartialEq, Archive, Serialize, Deserialize)]
pub struct MechanicalState {
    magazines: Vec<MagazineInstance>,
    inserted: Option<MagazineInstanceId>,
    chamber: Option<DefinitionId>,
    bolt_open: bool,
    equipped: bool,
    mode: FireMode,
    automatic: bool,
    action: Option<Manipulation>,
    action_sequence: u64,
}

impl MechanicalState {
    /// Initial total is four magazine capacities: one round moves from the
    /// inserted magazine into the chamber. It is never granted as a free +1.
    pub fn new(
        catalog: &Catalog,
        spec: &ResolvedWeapon,
        ids: [MagazineInstanceId; MAX_RETAINED_MAGAZINES],
    ) -> Result<Self, CatalogError> {
        let definition = spec
            .assembly
            .parts
            .iter()
            .find_map(|part| {
                catalog
                    .component(part.definition)
                    .filter(|c| c.kind == ComponentKind::Magazine)
            })
            .ok_or_else(|| {
                error(
                    CATALOG_FILE,
                    "mechanics.magazine",
                    "missing resolved magazine",
                )
            })?;
        let mut magazines: Vec<_> = ids
            .into_iter()
            .map(|id| MagazineInstance {
                id,
                definition: definition.id,
                ammo: spec.ammo,
                capacity: definition.capacity,
                rounds: definition.capacity,
            })
            .collect();
        magazines[0].rounds -= 1;
        let state = Self {
            inserted: Some(magazines[0].id),
            magazines,
            chamber: Some(spec.ammo),
            bolt_open: false,
            equipped: true,
            mode: if spec.automatic {
                FireMode::Auto
            } else {
                FireMode::Semi
            },
            automatic: spec.automatic,
            action: None,
            action_sequence: 0,
        };
        state.validate(catalog, spec)?;
        Ok(state)
    }

    /// Validate restored state before exposing it to simulation. A decoded
    /// archive is structurally safe but still needs these semantic checks.
    pub fn validate(&self, catalog: &Catalog, spec: &ResolvedWeapon) -> Result<(), CatalogError> {
        let fail = |message| error(CATALOG_FILE, "mechanics", message);
        let weapon = catalog
            .definition(spec.definition)
            .ok_or_else(|| fail("unknown weapon"))?;
        let base = spec
            .assembly
            .parts
            .iter()
            .find_map(|p| {
                catalog
                    .component(p.definition)
                    .filter(|c| c.kind == ComponentKind::Magazine)
            })
            .ok_or_else(|| fail("missing resolved magazine"))?;
        if self.magazines.is_empty() || self.magazines.len() > MAX_RETAINED_MAGAZINES {
            return Err(fail("invalid retained magazine count"));
        }
        let mut ids = std::collections::BTreeSet::new();
        for magazine in &self.magazines {
            let definition = catalog
                .component(magazine.definition)
                .ok_or_else(|| fail("unknown magazine definition"))?;
            if magazine.id.0 == 0
                || !ids.insert(magazine.id)
                || definition.kind != ComponentKind::Magazine
                || magazine.capacity != definition.capacity
                || magazine.ammo != definition.ammo
                || magazine.ammo != spec.ammo
                || definition.assembly.interface != base.assembly.interface
                || definition
                    .assembly
                    .platform
                    .as_ref()
                    .is_some_and(|p| *p != weapon.assembly.platform)
                || definition
                    .assembly
                    .caliber
                    .as_ref()
                    .is_some_and(|c| *c != weapon.assembly.caliber)
                || magazine.rounds > magazine.capacity
            {
                return Err(fail("invalid magazine identity/capacity/content"));
            }
        }
        if self.inserted.is_some_and(|id| !ids.contains(&id))
            || self.chamber.is_some_and(|round| round != spec.ammo)
            || (self.chamber.is_some() && self.bolt_open)
            || (self.mode == FireMode::Auto && !self.automatic)
            || self.automatic != spec.automatic
        {
            return Err(fail("inconsistent inserted magazine/chamber/selector"));
        }
        if let Some(action) = &self.action {
            if action.id == 0
                || action.id != self.action_sequence
                || !action.duration.is_finite()
                || !(0.01..=60.0).contains(&action.duration)
                || !action.elapsed.is_finite()
                || action.elapsed < 0.0
                || action.elapsed >= action.phase_duration()
            {
                return Err(fail("invalid manipulation clock or identity"));
            }
            let phase_matches = match action.kind {
                ManipulationKind::TacticalReload => matches!(
                    action.phase,
                    ManipulationPhase::Remove
                        | ManipulationPhase::Insert
                        | ManipulationPhase::Finish
                ),
                ManipulationKind::EmptyReload => matches!(
                    action.phase,
                    ManipulationPhase::Remove
                        | ManipulationPhase::Insert
                        | ManipulationPhase::Feed
                        | ManipulationPhase::Finish
                ),
                ManipulationKind::Equip => action.phase == ManipulationPhase::Equip,
                ManipulationKind::Unequip => action.phase == ManipulationPhase::Unequip,
                ManipulationKind::Inspect => action.phase == ManipulationPhase::Inspect,
            };
            if !phase_matches || action.target_magazine.is_some_and(|id| !ids.contains(&id)) {
                return Err(fail("invalid manipulation phase/target"));
            }
            if (action.kind == ManipulationKind::Equip) == self.equipped {
                return Err(fail("manipulation disagrees with equipped state"));
            }
            if matches!(
                action.kind,
                ManipulationKind::TacticalReload | ManipulationKind::EmptyReload
            ) {
                let Some(target) = action.target_magazine else {
                    return Err(fail("reload without target"));
                };
                if (action.phase == ManipulationPhase::Remove
                    && (self.inserted.is_none() || self.inserted == Some(target)))
                    || (action.phase == ManipulationPhase::Insert && self.inserted.is_some())
                    || (matches!(
                        action.phase,
                        ManipulationPhase::Feed | ManipulationPhase::Finish
                    ) && self.inserted != Some(target))
                {
                    return Err(fail("reload commit disagrees with magazine location"));
                }
                let chamber_expected = action.kind == ManipulationKind::TacticalReload
                    || action.phase == ManipulationPhase::Finish;
                if self.chamber.is_some() != chamber_expected {
                    return Err(fail("reload phase disagrees with chamber"));
                }
                if action.phase != ManipulationPhase::Finish
                    && self
                        .magazines
                        .iter()
                        .find(|m| m.id == target)
                        .unwrap()
                        .rounds
                        == 0
                {
                    return Err(fail("reload target is empty"));
                }
            } else if action.target_magazine.is_some() {
                return Err(fail("non-reload action has magazine target"));
            }
        }
        Ok(())
    }

    pub fn magazines(&self) -> &[MagazineInstance] {
        &self.magazines
    }
    pub fn inserted(&self) -> Option<&MagazineInstance> {
        self.magazines.iter().find(|m| Some(m.id) == self.inserted)
    }
    pub fn chamber(&self) -> Option<DefinitionId> {
        self.chamber
    }
    pub fn bolt_open(&self) -> bool {
        self.bolt_open
    }
    pub fn equipped(&self) -> bool {
        self.equipped
    }
    pub fn mode(&self) -> FireMode {
        self.mode
    }
    pub fn action(&self) -> Option<&Manipulation> {
        self.action.as_ref()
    }
    pub fn action_sequence(&self) -> u64 {
        self.action_sequence
    }
    pub fn loaded_rounds(&self) -> u32 {
        self.inserted().map_or(0, |m| m.rounds) + u32::from(self.chamber.is_some())
    }
    pub fn reserve_rounds(&self) -> u32 {
        self.magazines
            .iter()
            .filter(|m| Some(m.id) != self.inserted)
            .map(|m| m.rounds)
            .sum()
    }
    pub fn total_rounds(&self) -> u32 {
        self.loaded_rounds() + self.reserve_rounds()
    }

    pub fn cycle_selector(&mut self) {
        self.mode = match self.mode {
            FireMode::Safe => FireMode::Semi,
            FireMode::Semi if self.automatic => FireMode::Auto,
            _ => FireMode::Safe,
        };
    }

    fn begin(
        &mut self,
        kind: ManipulationKind,
        phase: ManipulationPhase,
        duration: f32,
        target_magazine: Option<MagazineInstanceId>,
    ) -> bool {
        if !duration.is_finite() || !(0.01..=60.0).contains(&duration) {
            return false;
        }
        let Some(id) = self.action_sequence.checked_add(1) else {
            return false;
        };
        self.action_sequence = id;
        self.action = Some(Manipulation {
            id,
            kind,
            phase,
            elapsed: 0.0,
            duration,
            target_magazine,
        });
        true
    }

    pub fn start_reload(&mut self, duration: f32) -> bool {
        if !self.equipped
            || self
                .action
                .as_ref()
                .is_some_and(|a| a.kind != ManipulationKind::Inspect)
        {
            return false;
        }
        let empty = self.chamber.is_none();
        let target = self
            .magazines
            .iter()
            .filter(|m| Some(m.id) != self.inserted && m.rounds > 0)
            .max_by_key(|m| (m.rounds, std::cmp::Reverse(m.id)))
            .map(|m| m.id);
        // If insertion committed before interruption, resume feed instead of
        // unnecessarily removing the already seated full magazine.
        let feed_only = empty && self.inserted().is_some_and(|m| m.rounds > 0);
        let (phase, target) = if feed_only {
            (ManipulationPhase::Feed, self.inserted)
        } else {
            let Some(target) = target else {
                return false;
            };
            let candidate = self.magazines.iter().find(|m| m.id == target).unwrap();
            if !empty
                && self
                    .inserted()
                    .is_some_and(|m| m.rounds >= candidate.rounds)
            {
                return false;
            }
            (
                if self.inserted.is_some() {
                    ManipulationPhase::Remove
                } else {
                    ManipulationPhase::Insert
                },
                Some(target),
            )
        };
        self.begin(
            if empty {
                ManipulationKind::EmptyReload
            } else {
                ManipulationKind::TacticalReload
            },
            phase,
            duration,
            target,
        )
    }

    pub fn start_inspect(&mut self, duration: f32) -> bool {
        if !self.equipped || self.action.is_some() {
            return false;
        }
        self.begin(
            ManipulationKind::Inspect,
            ManipulationPhase::Inspect,
            duration,
            None,
        )
    }

    pub fn start_unequip(&mut self, duration: f32) -> bool {
        if !self.equipped {
            return false;
        }
        // Higher priority than inspect/reload. Already committed magazine/chamber
        // state is retained when the action is replaced.
        self.begin(
            ManipulationKind::Unequip,
            ManipulationPhase::Unequip,
            duration,
            None,
        )
    }

    pub fn start_equip(&mut self, duration: f32) -> bool {
        if self.equipped || self.action.is_some() {
            return false;
        }
        self.begin(
            ManipulationKind::Equip,
            ManipulationPhase::Equip,
            duration,
            None,
        )
    }

    /// Cancel only uncommitted work. Magazine removal/insertion/feed remain real.
    pub fn cancel(&mut self) {
        self.action = None;
    }

    pub fn advance(&mut self, delta: f32) {
        if !delta.is_finite() || delta <= 0.0 {
            return;
        }
        let mut remaining = delta;
        while let Some(mut action) = self.action.take() {
            let advanced = action.elapsed + remaining;
            let duration = action.phase_duration();
            if advanced < duration {
                action.elapsed = advanced;
                self.action = Some(action);
                return;
            }
            remaining = advanced - duration;
            action.elapsed = 0.0;
            match action.phase {
                ManipulationPhase::Remove => {
                    self.inserted = None;
                    action.phase = ManipulationPhase::Insert;
                }
                ManipulationPhase::Insert => {
                    self.inserted = action.target_magazine;
                    action.phase = if action.kind == ManipulationKind::EmptyReload {
                        ManipulationPhase::Feed
                    } else {
                        ManipulationPhase::Finish
                    };
                }
                ManipulationPhase::Feed => {
                    self.feed();
                    action.phase = ManipulationPhase::Finish;
                }
                ManipulationPhase::Equip => {
                    self.equipped = true;
                    return;
                }
                ManipulationPhase::Unequip => {
                    self.equipped = false;
                    return;
                }
                ManipulationPhase::Finish | ManipulationPhase::Inspect => return,
            }
            self.action = Some(action);
        }
    }

    fn feed(&mut self) {
        if self.chamber.is_some() {
            return;
        }
        if let Some(magazine) = self
            .magazines
            .iter_mut()
            .find(|m| Some(m.id) == self.inserted && m.rounds > 0)
        {
            magazine.rounds -= 1;
            self.chamber = Some(magazine.ammo);
            self.bolt_open = false;
        } else {
            self.bolt_open = true;
        }
    }

    /// Outer Arsenal owns cadence and input sequence deduplication.
    pub fn fire(&mut self, held: bool, pressed: bool) -> bool {
        let trigger = match self.mode {
            FireMode::Safe => false,
            FireMode::Semi => pressed,
            FireMode::Auto => held,
        };
        if !trigger || !self.equipped {
            return false;
        }
        if self
            .action
            .as_ref()
            .is_some_and(|a| a.kind == ManipulationKind::Inspect)
        {
            self.cancel();
        }
        if self.action.is_some() || self.bolt_open || self.chamber.is_none() {
            return false;
        }
        self.chamber = None;
        self.feed();
        true
    }
}

#[cfg(test)]
mod tests;
