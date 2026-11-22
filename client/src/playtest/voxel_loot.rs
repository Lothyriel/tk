//! Optional normal-input route; the server never stages player or item poses.
use bevy::prelude::*;
use bevy_renet2::prelude::RenetClient;
use common::{
    ClientData, ClientInput, PLAYER_VIEW_HEIGHT,
    catalog::{
        ItemInstanceId,
        item_manifest::{S1_SEEDS, item_visuals},
    },
    replication::inventory::CHANNEL,
};

use crate::{
    input::inventory_panel::{Inspection, Panel},
    sync::inventory::State,
};

pub(super) fn enabled() -> bool {
    std::env::var_os("TK_PLAYTEST_VOXEL_LOOT").is_some()
}

pub(super) fn inspection_enabled() -> bool {
    std::env::var_os("TK_PLAYTEST_LOOT_INSPECT").is_some()
}

const VIEWS: [(&str, i8); 3] = [("front", 0), ("side", 3), ("back", 6)];
type Shown = (u32, ItemInstanceId, i8);

#[derive(Debug, Default)]
pub(super) struct Route {
    index: usize,
    dropped: bool,
    approach: usize,
    return_leg: usize,
    capture: Option<usize>,
    capture_until: f64,
    pickup_id: Option<ItemInstanceId>,
    inspection_view: usize,
    inspection_until: f64,
    inspection_capture: Option<String>,
}

impl Route {
    pub(super) fn take_capture(&mut self) -> Option<usize> {
        self.capture.take()
    }

    pub(super) fn take_inspection_capture(&mut self) -> Option<String> {
        self.inspection_capture.take()
    }

    /// Stage the same Panel state that the live inventory inspection consumes.
    /// Wait for the actual view before requesting a framebuffer, then keep the
    /// pose on screen while the normal capture queue submits the readback.
    fn inspect(
        &mut self,
        panel: &mut Panel,
        shown: Option<Shown>,
        raid: u32,
        seed: usize,
        id: ItemInstanceId,
        now: f64,
    ) -> bool {
        assert_eq!(
            self.pickup_id,
            Some(id),
            "inspection must use the acquired ground ID"
        );
        let Some(&(name, yaw_steps)) = VIEWS.get(self.inspection_view) else {
            panel.open = false;
            panel.inspection = None;
            panel.selected = None;
            return true;
        };
        panel.open = true;
        panel.selected = Some(id);
        panel.inspection = Some(Inspection {
            item: id,
            yaw_steps,
            pitch_steps: 0,
            zoom_steps: 0,
        });
        if self.inspection_until == 0.0 {
            if shown == Some((raid, id, yaw_steps)) {
                self.inspection_until = now + 0.18;
                self.inspection_capture = Some(format!("loot-held-{seed}-{name}"));
                info!("VOXEL INSPECT seed={seed} id={} view={name}", id.0);
            }
        } else if now >= self.inspection_until {
            self.inspection_until = 0.0;
            self.inspection_view += 1;
        }
        false
    }

    pub(super) fn tick(
        &mut self,
        input: &mut ClientInput,
        actor: &ClientData,
        inventory: &State,
        network: &mut RenetClient,
        (raid, peer, now): (u32, bool, f64),
        inspection: Option<(&mut Panel, Option<Shown>)>,
    ) -> bool {
        let dual = std::env::var("TK_PLAYTEST_CLIENTS").is_ok_and(|n| n == "2");
        let order: &[usize] = match (dual, peer) {
            (false, _) => &[0, 1, 2, 3, 5, 4],
            (true, false) => &[0, 1, 4],
            (true, true) => &[2, 3, 5],
        };
        let Some(grid) = inventory.owned_grid(raid, actor.id) else {
            return false;
        };
        if self.index == order.len() {
            let lane: f32 = if peer { 2.8 } else { -2.8 };
            while let Some(target) = [
                Vec3::new(lane, 0.0, -11.0),
                Vec3::new(lane, 0.0, -0.8),
                Vec3::new(lane.signum() * 3.6, 0.0, -0.8),
            ]
            .get(self.return_leg)
            {
                if !travel(input, actor, *target) {
                    return false;
                }
                self.return_leg += 1;
            }
            return true;
        }
        let seed = order[self.index];
        let definition = S1_SEEDS[seed];
        if let Some((id, _)) = grid
            .items
            .iter()
            .find(|(_, item)| item.definition == definition)
        {
            if let Some((panel, shown)) = inspection
                && !self.inspect(panel, shown, raid, seed, *id, now)
            {
                return false;
            }
            if self.index + 1 == order.len() {
                if actor.participant.carried == 1 {
                    info!("VOXEL LOOT retained seed={seed} id={}", id.0);
                    self.index += 1;
                }
            } else if !self.dropped
                && let Some(request) = inventory.drop_request(raid, actor.id, *id)
                && let Ok(bytes) = request.encode()
                && network.can_send_message(CHANNEL, bytes.len())
            {
                info!("VOXEL LOOT collected seed={seed} id={}", id.0);
                network.send_message(CHANNEL, bytes);
                self.dropped = true;
            }
            return false;
        }
        let Some(item) = inventory
            .ground()
            .0
            .iter()
            .find(|item| usize::from(item.seed) == seed)
        else {
            return false;
        };
        let center = Vec3::from_array(item.root)
            + Vec3::from_array(item_visuals()[definition].collider_center());
        if self.dropped {
            info!("VOXEL LOOT dropped seed={seed} id={}", item.id.0);
            self.index += 1;
            self.dropped = false;
            self.approach = 0;
            self.capture_until = 0.0;
            self.pickup_id = None;
            self.inspection_view = 0;
            return false;
        }
        if seed == 2 && self.approach == 0 {
            if !travel(input, actor, Vec3::new(3.2, 0.0, 0.8)) {
                return false;
            }
            self.approach += 1;
        }
        if seed >= 4 && self.approach == 0 {
            let lane = if peer || !dual { 2.8 } else { -2.8 };
            let z = if actor.pos[2] > -2.0 && lane < 0.0 {
                -1.6
            } else {
                -7.0
            };
            if !travel(input, actor, Vec3::new(lane, 0.0, z)) {
                return false;
            }
            self.approach += 1;
        }
        if seed >= 4 && self.approach == 1 {
            let lane = if peer || !dual { 2.8 } else { -2.8 };
            if !travel(input, actor, Vec3::new(lane, 0.0, -11.9)) {
                return false;
            }
            self.approach += 1;
        }
        if !travel(input, actor, center + Vec3::Z * 0.9) {
            return false;
        }
        let eye = Vec3::from_array(actor.pos) + Vec3::Y * PLAYER_VIEW_HEIGHT;
        let delta = center - eye;
        let recoil = actor.recoil.unpack();
        input.camera.yaw = (-delta.x).atan2(-delta.z) - recoil.angles[0];
        input.camera.pitch = delta.y.atan2(delta.xz().length()) - recoil.angles[1];
        if inventory
            .focused_ground(raid)
            .is_some_and(|focus| focus.id == item.id)
        {
            if std::env::var_os("TK_PLAYTEST_NO_CAPTURE").is_none() {
                if self.capture_until == 0.0 {
                    self.capture = Some(seed);
                    self.capture_until = now + 0.15;
                }
                if now < self.capture_until {
                    return false;
                }
            }
            input.interact = true;
            input.pickup_focus = Some((raid, item.id));
            self.pickup_id = Some(item.id);
        }
        false
    }
}

fn travel(input: &mut ClientInput, actor: &ClientData, target: Vec3) -> bool {
    let delta = target.xz() - Vec3::from_array(actor.pos).xz();
    if delta.length() <= 0.35 {
        return true;
    }
    super::move_toward(input, actor.pos, target.to_array());
    // The normal driver's 30 cm per-axis dead zone is wider than the
    // diagonal stopping radius; use a narrower per-axis margin here.
    input.left = delta.x < -0.2;
    input.right = delta.x > 0.2;
    input.forward = delta.y < -0.2;
    input.backward = delta.y > 0.2;
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loot_inspection_requires_matching_presented_pose_for_each_capture() {
        let id = ItemInstanceId(17);
        let mut route = Route {
            pickup_id: Some(id),
            ..default()
        };
        let mut panel = Panel::default();
        for (index, (label, yaw)) in VIEWS.into_iter().enumerate() {
            let now = 1.0 + index as f64;
            assert!(!route.inspect(
                &mut panel,
                Some((2, ItemInstanceId(18), yaw)),
                2,
                4,
                id,
                now
            ));
            assert!(route.take_inspection_capture().is_none());
            assert!(!route.inspect(&mut panel, Some((2, id, yaw)), 2, 4, id, now));
            assert_eq!(panel.inspection.unwrap().item, id);
            assert_eq!(panel.inspection.unwrap().yaw_steps, yaw);
            assert_eq!(
                route.take_inspection_capture(),
                Some(format!("loot-held-4-{label}"))
            );
            assert!(!route.inspect(&mut panel, Some((2, id, yaw)), 2, 4, id, now + 0.1));
            assert!(route.take_inspection_capture().is_none());
            assert!(!route.inspect(&mut panel, Some((2, id, yaw)), 2, 4, id, now + 0.2));
        }
        assert!(route.inspect(&mut panel, None, 2, 4, id, 5.0));
        assert!(!panel.open && panel.inspection.is_none() && panel.selected.is_none());
    }
}
