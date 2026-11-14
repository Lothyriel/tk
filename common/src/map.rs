//! One map definition for authoritative colliders and client presentation.
use crate::combat::SurfaceKind;

pub const SPAWN_CENTER: [f32; 3] = [0.0, 1.05, 10.0];
pub const EXTRACTION_CENTER: [f32; 3] = [0.0, 0.12, 15.0];
pub const LOOT_POSITIONS: [[f32; 3]; 3] =
    [[-4.0, 0.55, 0.0], [4.0, 0.55, -6.0], [0.0, 0.55, -13.0]];
pub const BOT_POSITIONS: [[f32; 3]; 2] = [[0.0, 1.05, -10.0], [8.0, 1.05, -6.0]];

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
