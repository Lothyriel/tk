//! One map definition for authoritative colliders and client presentation.
use crate::combat::SurfaceKind;

pub mod destruction;

pub const SPAWN_CENTER: [f32; 3] = [0.0, 1.05, 10.0];
pub const EXTRACTION_CENTER: [f32; 3] = [0.0, 0.12, 15.0];
pub const LOOT_POSITIONS: [[f32; 3]; 3] =
    [[-4.0, 0.55, 0.0], [4.0, 0.55, -6.0], [0.0, 0.55, -13.0]];
pub const BOT_POSITIONS: [[f32; 3]; 2] = [[0.0, 1.05, -10.0], [8.0, 1.05, -6.0]];

/// Static geometry for the opt-in POV fixture. Offset from the straight
/// approach so the step stays visible without intersecting either dual lane
/// (spawn centers at ±0.75 m, plus the character capsule radius).
pub const POV_STEP: MapBlock = MapBlock {
    center: [-1.55, 0.19, -1.75],
    half_size: [0.35, 0.09, 0.32],
    surface: SurfaceKind::Concrete,
};
pub const POV_MARKER: [f32; 3] = [0.0, 1.02, -2.392];

pub fn pov_side_cover() -> impl Iterator<Item = MapBlock> {
    let enabled = std::env::var("TK_PLAYTEST_POV").as_deref() == Ok("side-cover");
    [-1.0, 1.0]
        .into_iter()
        .filter(move |_| enabled)
        .map(|side| MapBlock {
            center: [side * 0.47, 1.15, -1.45],
            half_size: [0.10, 1.05, 0.70],
            surface: SurfaceKind::Concrete,
        })
}

pub fn pov_fixture() -> bool {
    matches!(
        std::env::var("TK_PLAYTEST_POV").as_deref(),
        Ok("ads" | "inspect" | "stop-lean" | "side-cover")
    )
}

pub struct MapBlock {
    pub center: [f32; 3],
    pub half_size: [f32; 3],
    pub surface: SurfaceKind,
}

pub const BLOCKS: &[MapBlock] = &[
    MapBlock {
        center: [0.0, 0.0, 0.0],
        half_size: [20.0, 0.1, 20.0],
        surface: SurfaceKind::Concrete,
    },
    MapBlock {
        center: [-20.0, 1.5, 0.0],
        half_size: [0.2, 1.5, 20.0],
        surface: SurfaceKind::Concrete,
    },
    MapBlock {
        center: [20.0, 1.5, 0.0],
        half_size: [0.2, 1.5, 20.0],
        surface: SurfaceKind::Concrete,
    },
    MapBlock {
        center: [0.0, 1.5, -20.0],
        half_size: [20.0, 1.5, 0.2],
        surface: SurfaceKind::Concrete,
    },
    MapBlock {
        center: [0.0, 1.5, 20.0],
        half_size: [20.0, 1.5, 0.2],
        surface: SurfaceKind::Concrete,
    },
    MapBlock {
        center: [0.0, 0.65, -3.0],
        half_size: [2.0, 0.55, 0.6],
        surface: SurfaceKind::Concrete,
    },
    MapBlock {
        center: [-7.0, 1.1, -5.0],
        half_size: [2.0, 1.0, 0.12],
        surface: SurfaceKind::Wood,
    },
    MapBlock {
        center: [5.0, 1.1, -10.0],
        half_size: [1.5, 1.0, 0.12],
        surface: SurfaceKind::Wood,
    },
    MapBlock {
        center: [8.0, 0.65, 3.0],
        half_size: [1.0, 0.55, 1.0],
        surface: SurfaceKind::Concrete,
    },
];

/// Full destruction state fits in 22 bytes, preserving the snapshot budget.
pub const BARRIER_CELL_COUNT: usize = 176;
pub type DestroyedCells = [u8; BARRIER_CELL_COUNT / 8];

pub fn destroyed_mask(ids: &[u16]) -> DestroyedCells {
    let mut mask = [0; BARRIER_CELL_COUNT / 8];
    for &id in ids {
        let id = id as usize;
        if id < BARRIER_CELL_COUNT {
            mask[id / 8] |= 1 << (id % 8);
        }
    }
    mask
}

pub fn destroyed_ids(mask: &DestroyedCells) -> Vec<u16> {
    (0..BARRIER_CELL_COUNT)
        .filter(|id| mask[id / 8] & (1 << (id % 8)) != 0)
        .map(|id| id as u16)
        .collect()
}

/// Interior cover only. The floor, arena boundary and loot stay indestructible.
pub fn destructible(block_index: usize) -> bool {
    (5..BLOCKS.len()).contains(&block_index)
}

#[derive(Debug, Clone)]
pub struct BarrierCell {
    pub id: u16,
    pub center: [f32; 3],
    pub half_size: [f32; 3],
    pub surface: SurfaceKind,
}

impl BarrierCell {
    pub fn health(&self) -> f32 {
        match self.surface {
            SurfaceKind::Wood => 160.0,
            SurfaceKind::Concrete => 100.0,
        }
    }
}

/// Stable iteration order is shared by authority and presentation, never client input.
pub fn barrier_cells() -> Vec<BarrierCell> {
    let mut cells = Vec::new();
    for (index, block) in BLOCKS.iter().enumerate() {
        if !destructible(index) {
            continue;
        }
        let counts = block
            .half_size
            .map(|half| (half * 2.0 / 0.5).ceil() as usize);
        let half_size = std::array::from_fn(|axis| block.half_size[axis] / counts[axis] as f32);
        for x in 0..counts[0] {
            for y in 0..counts[1] {
                for z in 0..counts[2] {
                    let offset = [x, y, z];
                    cells.push(BarrierCell {
                        id: cells.len() as u16,
                        center: std::array::from_fn(|axis| {
                            block.center[axis] - block.half_size[axis]
                                + half_size[axis] * (2 * offset[axis] + 1) as f32
                        }),
                        half_size,
                        surface: block.surface,
                    });
                }
            }
        }
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voxel_barriers_tile_only_interior_cover_with_stable_ids() {
        let cells = barrier_cells();
        assert_eq!(cells.len(), BARRIER_CELL_COUNT);
        assert_eq!(
            destroyed_ids(&destroyed_mask(&[0, 17, 175])),
            vec![0, 17, 175]
        );
        for (id, cell) in cells.iter().enumerate() {
            assert_eq!(cell.id as usize, id);
            assert_eq!(cell.center, barrier_cells()[id].center);
            assert!(cell.half_size.iter().all(|h| *h > 0.0 && *h <= 0.25));
        }
        for (index, block) in BLOCKS.iter().enumerate() {
            let volume: f32 = cells
                .iter()
                .filter(|cell| {
                    (0..3).all(|axis| {
                        (cell.center[axis] - block.center[axis]).abs() + cell.half_size[axis]
                            <= block.half_size[axis] + 0.0001
                    })
                })
                .map(|cell| cell.half_size.iter().product::<f32>() * 8.0)
                .sum();
            let expected = if destructible(index) {
                block.half_size.iter().product::<f32>() * 8.0
            } else {
                0.0
            };
            assert!(
                (volume - expected).abs() < 0.001,
                "block {index}: {volume} != {expected}"
            );
        }
    }
}
