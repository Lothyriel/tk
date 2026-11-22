//! Rendered loot QA only: coordinate two local drivers, never the authority.
use std::{fs, path::PathBuf, time::Duration};

use super::*;
use crate::{
    input::capture_pickup,
    render::{GroundModel, voxel::VoxelInstance},
};

pub(in crate::playtest) type ContestModels<'w, 's> = Query<
    'w,
    's,
    (
        &'static GroundModel,
        &'static InheritedVisibility,
        Has<VoxelInstance>,
    ),
>;

#[derive(Debug, Default)]
pub(in crate::playtest) struct Contest {
    target: Option<ItemInstanceId>,
    raid: Option<u32>,
    barrier: Option<PathBuf>,
    ready: bool,
    press: Option<(f64, (u32, ItemInstanceId))>,
}

impl Contest {
    pub(in crate::playtest) fn tick(
        &mut self,
        input: &mut ClientInput,
        actor: &ClientData,
        inventory: &State,
        models: &ContestModels,
        (raid, peer, now): (u32, bool, f64),
    ) -> bool {
        let Some(grid) = inventory.owned_grid(raid, actor.id) else {
            return false;
        };
        let ground = inventory.ground().0;
        let target = match self.target {
            Some(target) => target,
            None => {
                let Some(item) = ground.iter().find(|item| item.seed == 0) else {
                    return false;
                };
                assert!(grid.items.is_empty(), "contest starts before any pickup");
                self.target = Some(item.id);
                self.raid = Some(raid);
                self.barrier = Some(PathBuf::from(
                    std::env::var_os("TK_PLAYTEST_LOOT_CONTEST").expect("contest directory"),
                ));
                item.id
            }
        };
        assert_eq!(self.raid, Some(raid), "contest changed raids");
        let loose = ground.iter().find(|item| item.id == target);
        let model = models
            .iter()
            .find(|(model, _, _)| (model.0, model.1) == (u64::from(raid), target));
        if let Some((pressed_at, focus)) = self.press {
            // Sustain one ordinary key press across simulation ticks. No
            // retargeting or retry after the item leaves either client's focus.
            if now - pressed_at < 0.4 {
                input.pickup_focus = Some(focus);
                capture_pickup(input, Some(inventory), raid, true, false);
            }
            if now - pressed_at >= 0.75
                && loose.is_none()
                && model.is_none()
                && inventory.focused_ground(raid).is_none()
            {
                let owned = grid
                    .items
                    .iter()
                    .find(|(id, _)| *id == target)
                    .map_or(0, |(_, item)| item.quantity);
                if actor.participant.carried != u32::from(owned) {
                    return false;
                }
                assert_eq!(grid.items.len(), usize::from(owned > 0));
                info!(
                    "LOOT CONTEST converged raid={raid} item={} player={} owned={owned} ground=0 model=0",
                    target.0, actor.id
                );
                return true;
            }
            return false;
        }
        let item = loose.expect("contest item disappeared before this client pressed F");
        let visual = &item_visuals()[S1_SEEDS[0]];
        let center = Vec3::from_array(item.root) + Vec3::from_array(visual.collider_center());
        // Distinct approaches from the near side; neither actor should occupy
        // the other's sightline or the same stopping position.
        let offset = Vec3::new(if peer { 0.65 } else { -0.65 }, 0.0, 0.9);
        let arrived = travel(input, actor, center + offset);
        if arrived {
            aim_at(input, actor, center);
        }
        let focused = arrived
            && model.is_some_and(|(_, visible, assembled)| visible.get() && assembled)
            && inventory
                .focused_ground(raid)
                .is_some_and(|hit| hit.id == target);
        let barrier = self.barrier.as_ref().unwrap();
        let role = usize::from(peer);
        let ready = |role| barrier.join(format!("{raid}-{}-ready-{role}", target.0));
        if self.ready != focused {
            if focused {
                fs::write(ready(role), []).expect("write contest focus marker");
            } else {
                fs::remove_file(ready(role)).expect("remove lost contest focus marker");
            }
            self.ready = focused;
        }
        if !focused || !ready(1 - role).is_file() {
            return false;
        }
        let start = barrier.join(format!("{raid}-{}-start", target.0));
        if !peer && !start.exists() {
            fs::write(&start, []).expect("publish contest start");
        }
        // Use a shared host clock for the two driver frames, not for ticks or
        // delivery. Each client still requires current focus when it presses.
        if !fs::metadata(start)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|start| start.elapsed().ok())
            .is_some_and(|elapsed| elapsed >= Duration::from_millis(450))
        {
            return false;
        }
        capture_pickup(input, Some(inventory), raid, true, true);
        assert_eq!(input.pickup_focus, Some((raid, target)));
        self.press = input.pickup_focus.map(|focus| (now, focus));
        info!(
            "LOOT CONTEST pressed raid={raid} item={} player={}",
            target.0, actor.id
        );
        false
    }
}
