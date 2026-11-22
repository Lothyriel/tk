//! Deterministic raid rules. Live state is mutated by the server, snapshots are read by clients.
pub mod item_target;

use bevy::prelude::*;
use rkyv::{Archive, Deserialize, Serialize};

use crate::{
    combat::Loadout,
    map::{EXTRACTION_CENTER, LOOT_POSITIONS},
};

pub const COUNTDOWN_SECONDS: f32 = 3.0;
pub const RAID_SECONDS: f32 = 300.0;
pub const EXTRACTION_SECONDS: f32 = 5.0;
pub const EXTRACTION_RADIUS: f32 = 2.5;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub enum RaidPhase {
    #[default]
    Lobby,
    Countdown,
    Active,
    Debrief,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub enum RaidStatus {
    #[default]
    Waiting,
    Active,
    Extracted,
    Dead,
    Missing,
}

#[derive(Debug, Clone, Resource, Archive, Serialize, Deserialize)]
pub struct RaidState {
    pub phase: RaidPhase,
    pub seconds_left: f32,
    pub raid_id: u32,
    pub loot_available: [bool; 3],
}

impl Default for RaidState {
    fn default() -> Self {
        Self {
            phase: RaidPhase::Lobby,
            seconds_left: 0.0,
            raid_id: 0,
            loot_available: [true; 3],
        }
    }
}

impl RaidState {
    /// Returns true only on the tick that starts a new raid.
    pub fn advance_lobby(&mut self, ready: usize, total: usize, delta: f32) -> bool {
        if !matches!(self.phase, RaidPhase::Lobby | RaidPhase::Countdown) {
            return false;
        }
        if total == 0 || ready != total {
            self.phase = RaidPhase::Lobby;
            self.seconds_left = 0.0;
        } else if self.phase == RaidPhase::Lobby {
            self.phase = RaidPhase::Countdown;
            self.seconds_left = COUNTDOWN_SECONDS;
        } else {
            self.seconds_left = (self.seconds_left - delta).max(0.0);
            if self.seconds_left == 0.0 {
                self.phase = RaidPhase::Active;
                self.seconds_left = RAID_SECONDS;
                self.raid_id = self.raid_id.wrapping_add(1);
                self.loot_available = [true; 3];
                return true;
            }
        }
        false
    }

    pub fn advance_active(&mut self, delta: f32) -> bool {
        if self.phase != RaidPhase::Active {
            return false;
        }
        self.seconds_left = (self.seconds_left - delta).max(0.0);
        self.seconds_left == 0.0
    }

    pub fn finish_if_complete(&mut self, active_count: usize) {
        if self.phase == RaidPhase::Active && active_count == 0 {
            self.phase = RaidPhase::Debrief;
        }
    }

    pub fn reset_lobby(&mut self) {
        self.phase = RaidPhase::Lobby;
        self.seconds_left = 0.0;
    }
}

#[derive(Debug, Default, Clone, Component, Archive, Serialize, Deserialize)]
pub struct Participant {
    pub ready: bool,
    pub loadout: Loadout,
    pub status: RaidStatus,
    pub carried: u32,
    pub secured: u32,
    pub extraction_seconds: f32,
    pub last_command: u32,
}

impl Participant {
    pub fn begin(&mut self) {
        self.status = RaidStatus::Active;
        self.ready = false;
        self.carried = 0;
        self.extraction_seconds = 0.0;
    }

    pub fn return_to_lobby(&mut self) {
        self.status = RaidStatus::Waiting;
        self.ready = false;
        self.carried = 0;
        self.extraction_seconds = 0.0;
    }

    pub fn fail(&mut self, status: RaidStatus) {
        self.status = status;
        self.carried = 0;
        self.extraction_seconds = 0.0;
    }

    pub fn try_loot(&mut self, raid: &mut RaidState, position: Vec3, interact: bool) -> bool {
        if raid.phase != RaidPhase::Active || self.status != RaidStatus::Active || !interact {
            return false;
        }
        for (i, loot) in LOOT_POSITIONS.iter().enumerate() {
            if raid.loot_available[i] && position.distance(Vec3::from(*loot)) <= 2.0 {
                raid.loot_available[i] = false;
                self.carried += 1;
                return true;
            }
        }
        false
    }

    pub fn tick_extraction(
        &mut self,
        raid: &RaidState,
        position: Vec3,
        interact: bool,
        delta: f32,
    ) -> bool {
        let center = Vec3::from(EXTRACTION_CENTER);
        let horizontal = Vec2::new(position.x - center.x, position.z - center.z).length();
        let eligible = raid.phase == RaidPhase::Active
            && self.status == RaidStatus::Active
            && self.carried > 0
            && interact
            && horizontal <= EXTRACTION_RADIUS
            && (position.y - center.y).abs() <= 2.0;
        if !eligible {
            self.extraction_seconds = 0.0;
            return false;
        }
        self.extraction_seconds = (self.extraction_seconds + delta).min(EXTRACTION_SECONDS);
        if self.extraction_seconds < EXTRACTION_SECONDS {
            return false;
        }
        self.status = RaidStatus::Extracted;
        self.secured = self.secured.saturating_add(self.carried);
        self.carried = 0;
        true
    }
}

#[cfg(test)]
mod tests;
