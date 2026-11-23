//! Reliable shot delivery supplements the low-latency snapshot copy. IDs are
//! global for the server lifetime; config epochs do not restart this stream.
use std::collections::{BTreeSet, VecDeque};

use rkyv::{Archive, Deserialize, Serialize};

use crate::{data, FiredProjectileData, WorldSnapshot};

pub const SHOT_CHANNEL: u8 = 4;
pub const MAX_SNAPSHOT_BYTES: usize = 60_000;
// Leave room for the archived header with 72-byte protocol 17 shot records.
pub const MAX_SHOT_BATCH: usize = 112;
pub const MAX_SHOT_MESSAGE_BYTES: usize = 8_192;
pub const SHOT_WINDOW: u64 = 8_192;
pub const MAX_PENDING_SHOTS: usize = 8_192;
pub const PRESENT_SHOT_SECONDS: f32 = 0.15;

#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
pub enum ShotMessage {
    /// Sent once per authenticated connection, before any reliable shots.
    Bootstrap { first_id: u64 },
    Batch {
        raid: u32,
        time: f32,
        shots: Vec<FiredProjectileData>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum ShotError {
    PayloadSize,
    Archive,
    Bounds,
    Bootstrap,
    Backpressure,
}

impl ShotMessage {
    pub fn decode(bytes: &[u8]) -> Result<Self, ShotError> {
        if bytes.len() > MAX_SHOT_MESSAGE_BYTES {
            return Err(ShotError::PayloadSize);
        }
        let message: Self = data::try_decode(bytes).map_err(|_| ShotError::Archive)?;
        if let Self::Batch { time, shots, .. } = &message {
            if !time.is_finite()
                || *time < 0.0
                || shots.is_empty()
                || shots.len() > MAX_SHOT_BATCH
                || shots.iter().any(|shot| {
                    !shot.time.is_finite()
                        || shot.time < 0.0
                        || shot.time > *time
                        || crate::catalog::catalog().weapon(shot.weapon).is_none()
                        || !shot
                            .muzzle
                            .iter()
                            .chain(&shot.direction)
                            .chain(shot.ejection().iter().flatten())
                            .all(|v| v.is_finite())
                })
                || shots.windows(2).any(|pair| pair[0].id >= pair[1].id)
            {
                return Err(ShotError::Bounds);
            }
        }
        Ok(message)
    }
}

/// Sliding ID set: high-water alone loses a delayed shot when the snapshot
/// containing a newer ID wins the race with the reliable channel.
#[derive(Debug, Default)]
pub struct ShotWindow {
    first: u64,
    high: Option<u64>,
    seen: BTreeSet<u64>,
}

impl ShotWindow {
    pub fn starting_at(first: u64) -> Self {
        Self {
            first,
            ..Self::default()
        }
    }

    pub fn high(&self) -> Option<u64> {
        self.high
    }

    pub fn observe(&mut self, id: u64) -> bool {
        let floor = self
            .first
            .max(self.high.unwrap_or(0).saturating_sub(SHOT_WINDOW - 1));
        if id < floor || !self.seen.insert(id) {
            return false;
        }
        self.high = Some(self.high.map_or(id, |high| high.max(id)));
        let floor = self.high.unwrap().saturating_sub(SHOT_WINDOW - 1);
        while self.seen.first().is_some_and(|id| *id < floor) {
            self.seen.pop_first();
        }
        true
    }
}

#[derive(Debug)]
struct PendingBatch {
    raid: u32,
    time: f32,
    shots: Vec<FiredProjectileData>,
}

#[derive(Debug, Default)]
pub struct ShotReceiver {
    first: Option<u64>,
    window: ShotWindow,
    pending: VecDeque<PendingBatch>,
    pending_count: usize,
    pub stale: u64,
}

impl ShotReceiver {
    pub fn message(&mut self, bytes: &[u8]) -> Result<(), ShotError> {
        match ShotMessage::decode(bytes)? {
            ShotMessage::Bootstrap { first_id } => {
                if let Some(first) = self.first {
                    if first != first_id {
                        return Err(ShotError::Bootstrap);
                    }
                } else {
                    self.first = Some(first_id);
                    self.window = ShotWindow::starting_at(first_id);
                }
            }
            ShotMessage::Batch { raid, time, shots } => {
                if self.first.is_none() {
                    return Err(ShotError::Bootstrap);
                }
                if self.pending_count + shots.len() > MAX_PENDING_SHOTS {
                    return Err(ShotError::Backpressure);
                }
                self.pending_count += shots.len();
                self.pending.push_back(PendingBatch { raid, time, shots });
            }
        }
        Ok(())
    }

    pub fn snapshot(
        &mut self,
        shots: &[FiredProjectileData],
        now: f32,
        active: bool,
    ) -> Vec<FiredProjectileData> {
        // Reliable copies arrive after bootstrap; ignoring the fast copy before
        // that boundary prevents replay of pre-join history.
        if self.first.is_none() {
            return Vec::new();
        }
        shots
            .iter()
            .filter_map(|shot| self.consume(shot, now, active))
            .collect()
    }

    fn consume(
        &mut self,
        shot: &FiredProjectileData,
        now: f32,
        active: bool,
    ) -> Option<FiredProjectileData> {
        if !self.window.observe(shot.id) {
            return None;
        }
        if active && (0.0..PRESENT_SHOT_SECONDS).contains(&(now - shot.time)) {
            Some(shot.clone())
        } else {
            self.stale += 1;
            None
        }
    }

    pub fn drain(&mut self, raid: u32, now: f32, active: bool) -> Vec<FiredProjectileData> {
        let mut fresh = Vec::new();
        let mut waiting = VecDeque::new();
        while let Some(batch) = self.pending.pop_front() {
            if batch.raid > raid || batch.raid == raid && batch.time > now {
                waiting.push_back(batch);
                continue;
            }
            self.pending_count -= batch.shots.len();
            for shot in batch.shots {
                if let Some(shot) = self.consume(&shot, now, active && batch.raid == raid) {
                    fresh.push(shot);
                }
            }
        }
        self.pending = waiting;
        fresh
    }
}

#[derive(Debug)]
pub struct PackedSnapshot {
    pub bytes: bytes::Bytes,
    /// Caller must deliver these through the reliable stream, not discard them.
    pub overflow: Vec<FiredProjectileData>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SnapshotTooLarge {
    pub mandatory_bytes: usize,
}

/// Preserve all mandatory state. Overflow only the redundant shot history;
/// return the displaced records explicitly to make accidental loss visible.
pub fn pack_snapshot(mut snapshot: WorldSnapshot) -> Result<PackedSnapshot, SnapshotTooLarge> {
    let bytes = data::encode(&snapshot);
    if bytes.len() < MAX_SNAPSHOT_BYTES {
        return Ok(PackedSnapshot {
            bytes,
            overflow: Vec::new(),
        });
    }
    let mut overflow = Vec::new();
    let shot_size = std::mem::size_of::<crate::ArchivedFiredProjectileData>();
    let remove = (bytes.len() - (MAX_SNAPSHOT_BYTES - 1))
        .div_ceil(shot_size)
        .min(snapshot.fired_projectiles.len());
    overflow.extend(snapshot.fired_projectiles.drain(..remove));
    let bytes = data::encode(&snapshot);
    if bytes.len() >= MAX_SNAPSHOT_BYTES {
        return Err(SnapshotTooLarge {
            mandatory_bytes: bytes.len(),
        });
    }
    Ok(PackedSnapshot { bytes, overflow })
}

#[cfg(test)]
mod tests;
