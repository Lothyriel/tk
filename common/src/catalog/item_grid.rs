//! Shared geometry for validating inventory placement before committing a move.
//! Occupancy is derived from item locations, not maintained as a second inventory.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Footprint {
    width: u8,
    height: u8,
}

impl Footprint {
    pub fn new(width: u8, height: u8) -> Option<Self> {
        (width != 0 && height != 0).then_some(Self { width, height })
    }

    pub fn dimensions(self) -> [u8; 2] {
        [self.width, self.height]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum Rotation {
    Upright,
    QuarterTurn,
}

/// A top-left grid cell, measured from the compartment's origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Placement {
    pub x: u8,
    pub y: u8,
    pub rotation: Rotation,
}

impl Placement {
    pub fn area(self, footprint: Footprint) -> Area {
        let [width, height] = match self.rotation {
            Rotation::Upright => footprint.dimensions(),
            Rotation::QuarterTurn => [footprint.height, footprint.width],
        };
        Area {
            left: u16::from(self.x),
            top: u16::from(self.y),
            right: u16::from(self.x) + u16::from(width),
            bottom: u16::from(self.y) + u16::from(height),
        }
    }
}

/// Half-open rectangle; adjacent edges do not occupy the same cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    left: u16,
    top: u16,
    right: u16,
    bottom: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum PlacementError {
    OutsideGrid,
    Occupied,
}

impl Area {
    /// Check the proposed area against the other items in this compartment.
    /// For rotation or movement within a compartment, exclude the moving item.
    pub fn validate(
        self,
        grid: Footprint,
        other_items: impl IntoIterator<Item = Self>,
    ) -> Result<(), PlacementError> {
        if self.right > u16::from(grid.width) || self.bottom > u16::from(grid.height) {
            return Err(PlacementError::OutsideGrid);
        }
        if other_items.into_iter().any(|other| {
            self.left < other.right
                && other.left < self.right
                && self.top < other.bottom
                && other.top < self.bottom
        }) {
            return Err(PlacementError::Occupied);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_grid_rejects_empty_footprints_and_outside_coordinates_without_wrap() {
        assert_eq!(Footprint::new(0, 1), None);
        assert_eq!(Footprint::new(1, 0), None);
        let grid = Footprint::new(255, 255).unwrap();
        let mut cell = Placement {
            x: 254,
            y: 254,
            rotation: Rotation::Upright,
        };
        let item = Footprint::new(1, 1).unwrap();
        assert_eq!(cell.area(item).validate(grid, []), Ok(()));
        cell.x = 255;
        assert_eq!(
            cell.area(item).validate(grid, []),
            Err(PlacementError::OutsideGrid)
        );
    }

    #[test]
    fn item_grid_matches_cell_occupancy_oracle_for_both_rotations() {
        use std::collections::BTreeSet;
        let grid = Footprint::new(5, 4).unwrap();
        let existing = Placement {
            x: 1,
            y: 1,
            rotation: Rotation::Upright,
        };
        let fixed = Footprint::new(2, 2).unwrap();
        let taken = BTreeSet::from([(1, 1), (1, 2), (2, 1), (2, 2)]);
        for (rotation, offsets) in [
            (
                Rotation::Upright,
                [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)],
            ),
            (
                Rotation::QuarterTurn,
                [(0, 0), (1, 0), (0, 1), (1, 1), (0, 2), (1, 2)],
            ),
        ] {
            for x in 0..=5 {
                for y in 0..=4 {
                    let cells = offsets.map(|(dx, dy)| (x + dx, y + dy));
                    let expected = if cells.iter().any(|&(cx, cy)| cx >= 5 || cy >= 4) {
                        Err(PlacementError::OutsideGrid)
                    } else if cells.iter().any(|cell| taken.contains(cell)) {
                        Err(PlacementError::Occupied)
                    } else {
                        Ok(())
                    };
                    let area = Placement { x, y, rotation }.area(Footprint::new(3, 2).unwrap());
                    assert_eq!(
                        area.validate(grid, [existing.area(fixed)]),
                        expected,
                        "{x},{y} {rotation:?}"
                    );
                }
            }
        }
    }
}
