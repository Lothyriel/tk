//! Opt-in material review through ordinary movement, ammunition and cover updates.
use super::{capture, move_toward};
use bevy::prelude::*;
use common::{
    ClientData, ClientInput, EquipmentSlot,
    catalog::FireMode,
    map::{barrier_cells, destroyed_ids, destruction as shared},
};

pub(super) struct Scenario {
    pub name: &'static str,
    structure: u16,
    observer: Vec3,
    focus: Vec3,
}

impl Scenario {
    fn capture_aim(&self) -> Vec3 {
        // Include the upper wood corner and the floor where its chips settle.
        // Shot targets remain independent of this stable review camera.
        self.focus + Vec3::new(0.15, if self.structure == 6 { -0.9 } else { -0.2 }, 0.0)
    }
}

pub(super) fn scenario() -> Option<Scenario> {
    match std::env::var("TK_PLAYTEST_DESTRUCTION").as_deref() {
        Ok("wood") => Some(Scenario {
            name: "wood",
            structure: 6,
            observer: Vec3::new(-8.45, 1.0, -2.0),
            focus: Vec3::new(-8.93, 2.03, -4.88),
        }),
        Ok("concrete") => Some(Scenario {
            name: "concrete",
            structure: 5,
            observer: Vec3::new(-1.5, 1.0, -0.9),
            focus: Vec3::new(-1.96, 1.16, -2.4),
        }),
        _ => None,
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
enum Stage {
    #[default]
    Approach,
    Intact,
    FirstImpact,
    Degrade,
    Settle,
    Finish,
}

type FragmentSample = (u64, f64, Vec3, Quat);

#[derive(Default)]
pub(super) struct Review {
    stage: Stage,
    since: f64,
    initial_rounds: Option<u32>,
    first_removed: f32,
    degraded: bool,
    target: Option<Vec3>,
    target_rounds: u32,
    next_shot: f64,
    pressed_rounds: Option<u32>,
    next_report: f64,
    bootstrap_state: Option<(common::raid::RaidPhase, u32, u32, bool)>,
    cancelled_command: Option<u32>,
    bootstrap_since: Option<f64>,
    reload_requested: bool,
    selector: super::mechanics::Selector,
    fragment: Option<u32>,
    previous_pose: Option<FragmentSample>,
    candidates: std::collections::BTreeMap<u32, FragmentSample>,
    movement_seen: bool,
    still_since: Option<f64>,
}

impl Review {
    /// A real menu activation can arrive while the native window is warming up.
    /// Cancel that accepted ready command normally; never treat Countdown as Lobby.
    pub(super) fn bootstrap(
        &mut self,
        input: &mut ClientInput,
        local: &ClientData,
        raid: &common::raid::RaidState,
        now: f64,
    ) -> bool {
        use common::raid::RaidPhase;
        let participant = &local.participant;
        let observed = (
            raid.phase,
            input.lobby_sequence,
            participant.last_command,
            participant.ready,
        );
        if self.bootstrap_state != Some(observed) {
            info!(
                "DESTRUCTION BOOTSTRAP phase={:?} requested={} accepted={} ready={} mode={:?}",
                raid.phase,
                input.lobby_sequence,
                participant.last_command,
                participant.ready,
                raid.mode
            );
            self.bootstrap_state = Some(observed);
        }
        match raid.phase {
            RaidPhase::Lobby => {
                self.bootstrap_since = None;
                true
            }
            RaidPhase::Countdown => {
                assert!(
                    participant.ready,
                    "unexpected countdown without this solo reviewer ready"
                );
                assert!(
                    now - *self.bootstrap_since.get_or_insert(now) < 8.0,
                    "destruction bootstrap did not confirm Cancel-ready: requested={} accepted={} ready={}",
                    input.lobby_sequence,
                    participant.last_command,
                    participant.ready
                );
                if self.cancelled_command != Some(participant.last_command) {
                    if input.lobby_sequence == participant.last_command {
                        input.lobby_sequence = input.lobby_sequence.wrapping_add(1);
                    }
                    self.cancelled_command = Some(participant.last_command);
                    info!(
                        "DESTRUCTION BOOTSTRAP CANCEL requested={} accepted={}",
                        input.lobby_sequence, participant.last_command
                    );
                }
                false
            }
            RaidPhase::Active | RaidPhase::Debrief => panic!(
                "destruction bootstrap entered {:?} before confirmed equipment/deploy",
                raid.phase
            ),
        }
    }

    pub(super) fn advance(
        &mut self,
        commands: &mut Commands,
        input: &mut ClientInput,
        actor: (&ClientData, &common::catalog::ResolvedWeapon),
        snapshot: Option<&shared::CoverSnapshot>,
        elapsed: f64,
        scenario: &Scenario,
    ) -> bool {
        let (local, _) = actor;
        assert!(
            elapsed < 120.0,
            "destruction {} timed out at {:?}",
            scenario.name,
            self.stage
        );
        input.weapon = EquipmentSlot::Primary;
        let rounds = local.ammo_in_mag + local.reserve_ammo;
        let Some(snapshot) = snapshot else {
            return false;
        };
        let stats = summary(snapshot, scenario.structure);
        // Observe the same identity from release, including motion before the
        // fragment screenshot stage; returning to its first pose is not rest.
        let fragment_sampled = matches!(self.stage, Stage::Degrade | Stage::Settle)
            && self.observe_fragment(snapshot, scenario.structure, elapsed);
        match self.stage {
            Stage::Approach => {
                let distance = move_toward(input, local.pos, scenario.observer.into());
                // move_toward stops each axis within 0.3 m independently.
                if distance < 0.45 {
                    input.forward = false;
                    input.backward = false;
                    input.left = false;
                    input.right = false;
                    input.run = false;
                    if Vec3::from(local.pose.velocity).length() < 0.08 && local.pose.grounded {
                        self.stage = Stage::Intact;
                        self.since = elapsed;
                    }
                }
            }
            Stage::Intact if elapsed - self.since >= 1.0 => {
                assert!(
                    stats.removed < 1e-6 && stats.fragments == 0,
                    "review must start with intact material"
                );
                self.initial_rounds = Some(rounds);
                self.target_rounds = rounds;
                self.target = Some(scenario.focus);
                self.evidence(commands, scenario, "intact", snapshot, stats, 0);
                self.stage = Stage::FirstImpact;
                self.since = elapsed;
                self.next_shot = elapsed + 0.4;
            }
            Stage::FirstImpact => {
                assert!(
                    elapsed - self.since < 8.0 || self.first_removed > 0.0,
                    "first impact unconfirmed: rounds={rounds} mode={:?} action={:?} equipped={} fire={} press={} selector={} pos={:?} target={:?} cuts={}",
                    local.mechanics.mode,
                    local.mechanics.kind,
                    local.mechanics.equipped,
                    input.fire,
                    input.fire_pressed_sequence,
                    input.selector_sequence,
                    local.pos,
                    self.target,
                    snapshot.cuts.len()
                );
                let fired = self.initial_rounds.unwrap().saturating_sub(rounds);
                if fired == 0 {
                    self.shoot(input, actor, elapsed, rounds, scenario, snapshot);
                } else if stats.removed > 1e-8 {
                    if self.first_removed == 0.0 {
                        self.first_removed = stats.removed;
                        self.since = elapsed;
                    }
                    if elapsed - self.since >= 0.35 {
                        self.evidence(commands, scenario, "impact", snapshot, stats, fired);
                        self.stage = Stage::Degrade;
                        self.next_shot = elapsed + 0.4;
                    }
                }
            }
            Stage::Degrade => {
                let fired = self.initial_rounds.unwrap().saturating_sub(rounds);
                if !self.degraded && fired >= 5 && stats.removed > self.first_removed * 8.0 {
                    self.evidence(commands, scenario, "degraded", snapshot, stats, fired);
                    self.degraded = true;
                    self.next_shot = elapsed + 0.4;
                    return false;
                }
                if self.degraded
                    && elapsed >= self.next_shot
                    && snapshot
                        .rubble
                        .iter()
                        .any(|piece| Some(piece.id) == self.fragment)
                {
                    self.evidence(commands, scenario, "fragment", snapshot, stats, fired);
                    self.stage = Stage::Settle;
                    self.since = elapsed;
                } else {
                    assert!(
                        rounds > 0,
                        "destruction {} exhausted normal ammunition before real fragment evidence",
                        scenario.name
                    );
                    self.shoot(input, actor, elapsed, rounds, scenario, snapshot);
                }
            }
            Stage::Settle => {
                assert!(
                    elapsed - self.since < 10.0,
                    "destruction fragment did not visibly move and settle: id={:?} movement_seen={} still_since={:?} previous_pose={:?}",
                    self.fragment,
                    self.movement_seen,
                    self.still_since,
                    self.previous_pose
                );
                if fragment_sampled
                    && self.movement_seen
                    && self.still_since.is_some_and(|at| elapsed - at >= 0.8)
                {
                    self.evidence(
                        commands,
                        scenario,
                        "settled",
                        snapshot,
                        stats,
                        self.initial_rounds.unwrap().saturating_sub(rounds),
                    );
                    self.stage = Stage::Finish;
                }
            }
            _ => {}
        }
        self.stage == Stage::Finish
    }

    fn observe_fragment(
        &mut self,
        snapshot: &shared::CoverSnapshot,
        structure: u16,
        elapsed: f64,
    ) -> bool {
        let cells = barrier_cells();
        let mut pieces: Vec<_> = snapshot
            .rubble
            .iter()
            .filter(|p| cells[p.cell as usize].structure == structure)
            .collect();
        pieces.sort_unstable_by_key(|p| p.id);
        if let Some(id) = self.fragment {
            assert!(
                pieces.iter().any(|piece| piece.id == id),
                "reviewed fragment must retain its identity before settling"
            );
        }
        let mut selected_sampled = false;
        for piece in pieces {
            let current = (
                snapshot.sequence,
                elapsed,
                Vec3::from(piece.position),
                Quat::from_array(piece.rotation),
            );
            let Some(&(sequence, at, previous, rotation)) = self.candidates.get(&piece.id) else {
                self.candidates.insert(piece.id, current);
                info!(
                    "DESTRUCTION FRAGMENT START id={} sequence={} t={elapsed:.3} position={:?} rotation={:?} boxes={}",
                    piece.id,
                    snapshot.sequence,
                    piece.position,
                    piece.rotation,
                    piece.boxes.len()
                );
                continue;
            };
            if snapshot.sequence <= sequence || elapsed - at < 0.2 {
                continue;
            }
            let travel = previous.distance(current.2);
            let turn = rotation.angle_between(current.3);
            let dt = (elapsed - at) as f32;
            // Stable order selects the lowest ID whose own observed motion
            // meets acceptance. Once selected, even a lower moving ID cannot replace it.
            if self.fragment.is_none() && (travel > 0.005 || turn > 0.02) {
                self.fragment = Some(piece.id);
                self.movement_seen = true;
                info!(
                    "DESTRUCTION FRAGMENT SELECT id={} sequence={} t={elapsed:.3}",
                    piece.id, snapshot.sequence
                );
            }
            if self.fragment == Some(piece.id) {
                selected_sampled = true;
                self.previous_pose = Some(current);
                if travel / dt < 0.05 && turn / dt < 0.08 {
                    self.still_since.get_or_insert(elapsed);
                } else {
                    self.still_since = None;
                }
            }
            info!(
                "DESTRUCTION FRAGMENT POSE id={} selected={} sequence={} t={elapsed:.3} position={:?} rotation={:?}",
                piece.id,
                Some(piece.id) == self.fragment,
                snapshot.sequence,
                piece.position,
                piece.rotation
            );
            info!(
                "DESTRUCTION FRAGMENT MOTION id={} t={elapsed:.3} travel_m={travel:.6} turn_rad={turn:.6} dt={dt:.3} speed_mps={:.6} angular_rps={:.6} selected={} still_since={:?}",
                piece.id,
                travel / dt,
                turn / dt,
                Some(piece.id) == self.fragment,
                self.still_since
            );
            self.candidates.insert(piece.id, current);
        }
        selected_sampled
    }

    fn shoot(
        &mut self,
        input: &mut ClientInput,
        actor: (&ClientData, &common::catalog::ResolvedWeapon),
        elapsed: f64,
        rounds: u32,
        scenario: &Scenario,
        snapshot: &shared::CoverSnapshot,
    ) {
        let (local, spec) = actor;
        if self.target_rounds != rounds {
            self.target_rounds = rounds;
            self.target = next_target(
                scenario,
                snapshot,
                self.initial_rounds.unwrap().saturating_sub(rounds),
            );
            self.next_shot = elapsed + 0.2;
        }
        let target = self
            .target
            .expect("review must find remaining material, not shoot an empty hole");
        if elapsed >= self.next_shot {
            aim_installed(input, local, target, spec);
        }
        if local.mechanics.kind.is_some() || local.pose.ads > 0.0 {
            return;
        }
        // A delayed render/snapshot must not turn the isolated first impact
        // into an automatic burst while the driver waits for confirmation.
        self.selector.select(input, local, FireMode::Semi);
        if local.mechanics.mode != FireMode::Semi {
            return;
        }
        if local.ammo_in_mag == 0 {
            if !self.reload_requested {
                input.reload_sequence = input.reload_sequence.wrapping_add(1);
                self.reload_requested = true;
            }
        } else {
            self.reload_requested = false;
            input.fire = elapsed >= self.next_shot;
            if input.fire && self.pressed_rounds != Some(rounds) {
                input.fire_pressed_sequence = input.fire_pressed_sequence.wrapping_add(1);
                self.pressed_rounds = Some(rounds);
                info!(
                    "DESTRUCTION SHOT press={} target={target:?} look={:?} outlet={:?}",
                    input.fire_pressed_sequence,
                    input.camera,
                    common::weapon_animation::resolved_muzzle_offset(spec, 0.0)
                );
            }
        }
    }

    pub(super) fn report(
        &mut self,
        input: &ClientInput,
        local: &ClientData,
        snapshot: Option<&shared::CoverSnapshot>,
        elapsed: f64,
        eye: Vec3,
    ) {
        if elapsed < self.next_report {
            return;
        }
        self.next_report = elapsed + 1.0;
        info!(
            "DESTRUCTION STATE stage={:?} t={elapsed:.2} rounds={}/{} mode={:?} action={:?} equipped={} fire={} press={} selector={} pos={:?} eye={eye:?} look={:?} target={:?} cover_sequence={} cuts={}",
            self.stage,
            local.ammo_in_mag,
            local.reserve_ammo,
            local.mechanics.mode,
            local.mechanics.kind,
            local.mechanics.equipped,
            input.fire,
            input.fire_pressed_sequence,
            input.selector_sequence,
            local.pos,
            input.camera,
            self.target,
            snapshot.map_or(0, |s| s.sequence),
            snapshot.map_or(0, |s| s.cuts.len())
        );
    }

    fn evidence(
        &self,
        commands: &mut Commands,
        scenario: &Scenario,
        cue: &str,
        snapshot: &shared::CoverSnapshot,
        stats: Summary,
        shots: u32,
    ) {
        let name = format!("destruction-{}-{cue}", scenario.name);
        capture(commands, &name);
        info!(
            "DESTRUCTION EVIDENCE {name} shots={shots} removed_m3={:.7} fragments={} fragment_boxes={} cover_bytes={} sequence={}",
            stats.removed,
            stats.fragments,
            stats.boxes,
            common::data::encode(snapshot).len(),
            snapshot.sequence
        );
    }
}

/// Solve the hip-fire ray from the accepted assembly's actual installed outlet.
/// Use the accepted configuration rather than the catalog's default assembly.
fn aim_installed(
    input: &mut ClientInput,
    local: &ClientData,
    target: Vec3,
    spec: &common::catalog::ResolvedWeapon,
) {
    let eye = Vec3::from(local.pos)
        + Vec3::Y
            * (common::PLAYER_VIEW_HEIGHT
                + if local.crouched {
                    common::PLAYER_CROUCH_VIEW_OFFSET
                } else {
                    0.0
                });
    let d = target - eye;
    let b = common::weapon_animation::resolved_muzzle_offset(spec, 0.0);
    let h = (d.length_squared() - b.x * b.x - b.y * b.y)
        .max(0.0001)
        .sqrt();
    let pitch = (d.y / (h * h + b.y * b.y).sqrt()).clamp(-1.0, 1.0).asin() - b.y.atan2(h);
    let local_z = b.y * pitch.sin() - h * pitch.cos();
    let recoil = local.recoil.unpack();
    input.camera = common::CameraInput {
        pitch: pitch - recoil.angles[1],
        yaw: (-d.x).atan2(-d.z) + b.x.atan2(-local_z) - recoil.angles[0],
        roll: 0.0,
    };
}

#[derive(Clone, Copy)]
struct Summary {
    removed: f32,
    fragments: usize,
    boxes: usize,
}

fn summary(snapshot: &shared::CoverSnapshot, structure: u16) -> Summary {
    let cells = barrier_cells();
    let removed = destroyed_ids(&snapshot.removed);
    let lost = cells
        .iter()
        .filter(|cell| cell.structure == structure)
        .map(|cell| {
            let original = Vec3::from(cell.half_size).element_product() * 8.0;
            original
                - if removed.contains(&cell.id) {
                    0.0
                } else {
                    shared::solids(cell, &snapshot.cuts)
                        .iter()
                        .map(|part| part.volume())
                        .sum()
                }
        })
        .sum();
    let fragments: Vec<_> = snapshot
        .rubble
        .iter()
        .filter(|piece| cells[piece.cell as usize].structure == structure)
        .collect();
    Summary {
        removed: lost,
        fragments: fragments.len(),
        boxes: fragments.iter().map(|piece| piece.boxes.len()).sum(),
    }
}

fn next_target(scenario: &Scenario, snapshot: &shared::CoverSnapshot, shots: u32) -> Option<Vec3> {
    let removed = destroyed_ids(&snapshot.removed);
    let material: Vec<_> = barrier_cells()
        .iter()
        .filter(|cell| cell.structure == scenario.structure && !removed.contains(&cell.id))
        .flat_map(|cell| {
            shared::solids(cell, &snapshot.cuts)
                .into_iter()
                .map(move |part| part.translated(cell.center.into()))
        })
        .collect();
    for sample in 0..81 {
        let angle = (sample + shots * 17) as f32 * 2.3999631;
        let radius = 0.012 + (sample / 9) as f32 * 0.018;
        let x = scenario.focus.x + angle.cos() * radius;
        let y = scenario.focus.y + angle.sin() * radius;
        if let Some(z) = material
            .iter()
            .filter(|part| {
                (x - part.center.x).abs() < part.half_size.x - 1e-5
                    && (y - part.center.y).abs() < part.half_size.y - 1e-5
            })
            .map(|part| part.center.z + part.half_size.z)
            .max_by(f32::total_cmp)
        {
            return Some(Vec3::new(x, y, z));
        }
    }
    None
}

/// Presentation uses an anatomical eye in front of the head, while ballistic
/// aiming uses the authoritative neutral eye. Only screenshots use this look.
pub(super) fn capture_view(
    input: &mut ClientInput,
    local: &ClientData,
    eye: Vec3,
    scenario: &Scenario,
) {
    let delta = scenario.capture_aim() - eye;
    input.camera = common::CameraInput {
        yaw: (-delta.x).atan2(-delta.z),
        pitch: delta.y.atan2(delta.x.hypot(delta.z)),
        roll: 0.0,
    };
    let recoil = local.recoil.unpack();
    input.camera.yaw -= recoil.angles[0];
    input.camera.pitch -= recoil.angles[1];
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use common::{WeaponKind, map::destroyed_mask, raid::RaidPhase};

    fn wood() -> Scenario {
        Scenario {
            name: "wood",
            structure: 6,
            observer: Vec3::new(-8.45, 1.0, -2.0),
            focus: Vec3::new(-8.93, 2.03, -4.88),
        }
    }

    #[test]
    fn destruction_review_bootstrap_cancels_early_ready_once_and_requires_confirmed_lobby() {
        let mut review = Review::default();
        let mut input = ClientInput::default();
        let mut local = ClientData::default();
        let mut raid = common::raid::RaidState::default();
        assert!(review.bootstrap(&mut input, &local, &raid, 0.0));
        // The normal menu issued Ready before the review's equipment stage.
        input.lobby_sequence = 1;
        local.participant.last_command = 1;
        local.participant.ready = true;
        raid.advance_lobby(1, 1, 0.0);
        assert_eq!(raid.phase, RaidPhase::Countdown);
        assert!(!review.bootstrap(&mut input, &local, &raid, 0.1));
        assert_eq!(input.lobby_sequence, 2);
        for now in [0.2, 0.3, 0.4] {
            assert!(!review.bootstrap(&mut input, &local, &raid, now));
            assert_eq!(
                input.lobby_sequence, 2,
                "a delayed acknowledgement must not toggle ready repeatedly"
            );
        }
        local.participant.last_command = input.lobby_sequence;
        local.participant.ready = false;
        raid.advance_lobby(0, 1, 0.0);
        assert_eq!(raid.phase, RaidPhase::Lobby);
        assert!(review.bootstrap(&mut input, &local, &raid, 0.5));
        assert_eq!(input.lobby_sequence, 2);
    }

    #[test]
    #[should_panic(expected = "before confirmed equipment/deploy")]
    fn destruction_review_bootstrap_refuses_an_already_active_raid() {
        let mut review = Review::default();
        let mut input = ClientInput::default();
        let raid = common::raid::RaidState {
            phase: RaidPhase::Active,
            ..default()
        };
        review.bootstrap(&mut input, &ClientData::default(), &raid, 0.0);
    }

    #[test]
    fn destruction_review_aims_the_accepted_installed_outlet_at_actual_material() {
        let mut spec = common::WeaponId::M4A1.spec().clone();
        for transverse_change in [0.0, 0.07] {
            // An accepted kit can also differ from the catalog's default assembly.
            spec.assembly.hip_from_root.translation.x += transverse_change;
            for position in [wood().observer, Vec3::new(-8.597696, 1.0179349, -3.2066545)] {
                let local = ClientData {
                    pos: position.into(),
                    ..default()
                };
                let mut input = ClientInput::default();
                let target = wood().focus;
                aim_installed(&mut input, &local, target, &spec);
                let rotation = Quat::from(&input.camera);
                let eye = position + Vec3::Y * common::PLAYER_VIEW_HEIGHT;
                let muzzle =
                    eye + rotation * common::weapon_animation::resolved_muzzle_offset(&spec, 0.0);
                let direction = rotation * Vec3::NEG_Z;
                let distance = (target - muzzle).dot(direction);
                assert!(distance > 0.0);
                assert!((muzzle + direction * distance).distance(target) < 0.0001);
                let catalog_rotation =
                    Quat::from(&super::super::barrel_aim(eye, target, local.weapon_id));
                let catalog_muzzle = eye
                    + catalog_rotation
                        * common::weapon_animation::resolved_muzzle_offset(&spec, 0.0);
                let catalog_miss = (target - catalog_muzzle)
                    .cross(catalog_rotation * Vec3::NEG_Z)
                    .length();
                if transverse_change == 0.0 {
                    // The resolver already derives the default barrel offset from
                    // its authored bore. A longer device outlet shares that ray.
                    assert!(catalog_miss < 0.0001);
                } else {
                    assert!(
                        catalog_miss > 0.06,
                        "a changed accepted assembly must not use the catalog default"
                    );
                }
            }
        }
    }

    #[test]
    fn destruction_review_camera_contains_the_impact_and_settled_floor() {
        let scenario = wood();
        for x in [-0.3, 0.0, 0.3] {
            // Native approach coasts another ~0.11 m beyond its axis dead zone.
            for z in [-0.5, 0.0, 0.3] {
                for y in [1.0, 1.05] {
                    let player = ClientData {
                        pos: (scenario.observer + Vec3::new(x, y - scenario.observer.y, z)).into(),
                        ..default()
                    };
                    let mut input = ClientInput::default();
                    let presented_eye = |camera: common::CameraInput| {
                        Vec3::from(player.pos)
                            + Quat::from_rotation_y(camera.yaw)
                                * crate::render::humanoid_eye(
                                    Vec3::Y * (common::PLAYER_VIEW_HEIGHT + 0.10),
                                    Quat::from_rotation_x(camera.pitch),
                                )
                    };
                    for _ in 0..8 {
                        let eye = presented_eye(input.camera);
                        capture_view(&mut input, &player, eye, &scenario);
                    }
                    let eye = presented_eye(input.camera);
                    let rotation = Quat::from(&input.camera);
                    for point in [
                        scenario.focus + Vec3::Y * 0.17,
                        Vec3::new(scenario.focus.x, 0.03, scenario.focus.z),
                    ] {
                        let view = rotation.inverse() * (point - eye);
                        assert!(
                            view.z < -0.1
                                && view.y.abs() < -view.z * 35.0_f32.to_radians().tan() * 0.94,
                            "impact/floor outside stable70degree review: point={point:?}, eye={eye:?}, view={view:?}"
                        );
                    }
                }
            }
        }
    }

    #[derive(Resource)]
    struct Fixture {
        review: Review,
        input: ClientInput,
        player: ClientData,
        snapshot: shared::CoverSnapshot,
    }

    fn step(In(elapsed): In<f64>, mut commands: Commands, mut fixture: ResMut<Fixture>) {
        let Fixture {
            review,
            input,
            player,
            snapshot,
        } = &mut *fixture;
        review.advance(
            &mut commands,
            input,
            (player, player.weapon_id.spec()),
            Some(snapshot),
            elapsed,
            &wood(),
        );
    }

    #[test]
    fn destruction_review_waits_for_ammunition_and_changed_material_not_elapsed_time() {
        let mut world = World::new();
        let mut player = ClientData {
            pos: wood().observer.into(),
            ammo_in_mag: 30,
            reserve_ammo: 90,
            ..default()
        };
        player.pose.grounded = true;
        world.init_resource::<super::super::video::CaptureQueue>();
        world.insert_resource(Fixture {
            review: Review::default(),
            input: ClientInput::default(),
            player,
            snapshot: shared::CoverSnapshot {
                sequence: 1,
                raid_id: 1,
                phase: RaidPhase::Active,
                removed: destroyed_mask(&[]),
                cuts: vec![],
                rubble: vec![],
            },
        });
        for at in [0.0, 1.1, 2.0, 7.0] {
            world.run_system_once_with(step, at).unwrap();
        }
        assert_eq!(world.resource::<Fixture>().review.stage, Stage::FirstImpact);
        assert_eq!(
            world.resource::<super::super::video::CaptureQueue>().0,
            ["destruction-wood-intact"]
        );

        // Ammunition alone is insufficient: its round may have hit elsewhere,
        // or the independent cover update may still be late.
        world.resource_mut::<Fixture>().player.ammo_in_mag -= 1;
        world.run_system_once_with(step, 7.1).unwrap();
        assert_eq!(world.resource::<Fixture>().review.stage, Stage::FirstImpact);

        let scenario = wood();
        let cell = barrier_cells()
            .into_iter()
            .find(|cell| {
                cell.structure == scenario.structure
                    && (scenario.focus - Vec3::from(cell.center))
                        .abs()
                        .cmple(Vec3::from(cell.half_size) + Vec3::splat(1e-5))
                        .all()
            })
            .unwrap();
        world
            .resource_mut::<Fixture>()
            .snapshot
            .cuts
            .push(shared::impact_cut(
                &cell,
                scenario.focus,
                Vec3::Z,
                WeaponKind::Rifle,
            ));
        for at in [7.2, 7.7] {
            world.run_system_once_with(step, at).unwrap();
        }
        assert_eq!(world.resource::<Fixture>().review.stage, Stage::Degrade);
        assert_eq!(
            world.resource::<super::super::video::CaptureQueue>().0,
            ["destruction-wood-intact", "destruction-wood-impact"]
        );

        // More shots without further material loss cannot produce a later
        // screenshot merely because its allotted time has passed.
        world.resource_mut::<Fixture>().player.ammo_in_mag = 25;
        for at in [20.0, 40.0] {
            world.run_system_once_with(step, at).unwrap();
        }
        assert!(!world.resource::<Fixture>().review.degraded);
        assert_eq!(
            world
                .resource::<super::super::video::CaptureQueue>()
                .0
                .len(),
            2
        );
    }

    #[test]
    fn destruction_review_sends_one_semi_press_until_actual_ammunition_acknowledges_it() {
        let mut arsenal = common::combat::Arsenal::new(common::combat::Loadout::default());
        let scenario = wood();
        let initial = arsenal.current().rounds();
        let mut local = ClientData {
            pos: scenario.observer.into(),
            ammo_in_mag: initial,
            reserve_ammo: arsenal.current().reserve(),
            mechanics: arsenal.current().mechanics().snapshot(),
            ..default()
        };
        let rounds = local.ammo_in_mag + local.reserve_ammo;
        let mut review = Review {
            initial_rounds: Some(rounds),
            target_rounds: rounds,
            target: Some(scenario.focus),
            ..default()
        };
        let snapshot = shared::CoverSnapshot {
            sequence: 1,
            raid_id: 1,
            phase: RaidPhase::Active,
            removed: destroyed_mask(&[]),
            cuts: vec![],
            rubble: vec![],
        };
        let mut input = ClientInput::default();
        let mut shots = 0;
        for at in [0.0, 0.02, 0.04, 0.2, 0.4, 0.6] {
            input.fire = false;
            review.shoot(
                &mut input,
                (&local, local.weapon_id.spec()),
                at,
                rounds,
                &scenario,
                &snapshot,
            );
            shots += usize::from(arsenal.step(&input, 0.0, at as f32).is_some());
            // Mode updates arrive, but deliberately delay ammunition readback.
            local.mechanics = arsenal.current().mechanics().snapshot();
        }
        assert_eq!(
            shots, 1,
            "the ordinary server Arsenal must authorize the initial semi shot exactly once"
        );
        assert_eq!(
            input.fire_pressed_sequence, 1,
            "held/delayed confirmation must not manufacture more presses"
        );
        assert_eq!(arsenal.current().rounds(), initial - 1);
        local.ammo_in_mag = arsenal.current().rounds();
        let acknowledged = local.ammo_in_mag + local.reserve_ammo;
        for at in [1.0, 1.3] {
            input.fire = false;
            review.shoot(
                &mut input,
                (&local, local.weapon_id.spec()),
                at,
                acknowledged,
                &scenario,
                &snapshot,
            );
            shots += usize::from(arsenal.step(&input, 0.0, at as f32).is_some());
        }
        assert_eq!(
            shots, 2,
            "a fresh acknowledged shot must receive a fresh trigger press"
        );
        assert_eq!(input.fire_pressed_sequence, 2);
    }

    #[test]
    fn destruction_review_selects_moving_candidate_once_and_observes_early_return() {
        let cell = barrier_cells()
            .into_iter()
            .find(|c| c.structure == wood().structure)
            .unwrap();
        let mut snapshot = shared::CoverSnapshot {
            sequence: 1,
            raid_id: 1,
            phase: RaidPhase::Active,
            removed: destroyed_mask(&[]),
            cuts: vec![],
            rubble: [7, 8]
                .map(|id| shared::RubblePose {
                    id,
                    cell: cell.id,
                    position: [0.0; 3],
                    rotation: Quat::IDENTITY.to_array(),
                    boxes: vec![],
                })
                .into(),
        };
        let mut review = Review {
            stage: Stage::Degrade,
            ..default()
        };
        assert!(!review.observe_fragment(&snapshot, wood().structure, 0.0));
        for (sequence, at) in [(2, 0.25), (3, 0.5), (4, 0.75)] {
            snapshot.sequence = sequence;
            assert!(!review.observe_fragment(&snapshot, wood().structure, at));
            assert!(
                review.fragment.is_none(),
                "stationary chips cannot be selected"
            );
            assert!(!review.movement_seen);
            assert!(review.still_since.is_none());
        }
        snapshot.sequence = 5;
        snapshot.rubble[1].position[1] = 0.1;
        assert!(review.observe_fragment(&snapshot, wood().structure, 1.0));
        assert_eq!(
            review.fragment,
            Some(8),
            "the stationary first chip must not block a moving second chip"
        );
        snapshot.sequence = 6;
        snapshot.rubble[0].position[1] = 0.2;
        snapshot.rubble[1].position[1] = 0.0;
        assert!(review.observe_fragment(&snapshot, wood().structure, 1.25));
        assert_eq!(
            review.fragment,
            Some(8),
            "a later-moving lower ID cannot replace the selected chip"
        );
        assert!(
            review.movement_seen,
            "ejection and return before Settle must remain observed"
        );
        assert!(review.still_since.is_none());
        review.stage = Stage::Settle;
        snapshot.sequence = 7;
        assert!(review.observe_fragment(&snapshot, wood().structure, 1.5));
        assert_eq!(review.still_since, Some(1.5));
        assert!(
            !review.observe_fragment(&snapshot, wood().structure, 3.0),
            "elapsed time on a duplicate snapshot is not a fresh settling sample"
        );
        snapshot.sequence = 8;
        assert!(review.observe_fragment(&snapshot, wood().structure, 3.0));
        assert_eq!(review.still_since, Some(1.5));

        // Input vector order must not determine which simultaneously moving ID wins.
        let mut simultaneous = Review::default();
        snapshot.rubble.swap(0, 1);
        assert!(!simultaneous.observe_fragment(&snapshot, wood().structure, 0.0));
        snapshot.sequence += 1;
        for piece in &mut snapshot.rubble {
            piece.position[0] += 0.1;
        }
        assert!(simultaneous.observe_fragment(&snapshot, wood().structure, 0.25));
        assert_eq!(simultaneous.fragment, Some(7));
    }

    #[test]
    fn destruction_review_targets_remaining_material_and_distinguishes_fragment_ids() {
        let scenario = wood();
        let cell = barrier_cells()
            .into_iter()
            .find(|cell| cell.structure == scenario.structure)
            .unwrap();
        let mut snapshot = shared::CoverSnapshot {
            sequence: 1,
            raid_id: 1,
            phase: RaidPhase::Active,
            removed: destroyed_mask(&[]),
            cuts: vec![],
            rubble: [7, 8]
                .map(|id| shared::RubblePose {
                    id,
                    cell: cell.id,
                    position: [0.0; 3],
                    rotation: Quat::IDENTITY.to_array(),
                    boxes: vec![
                        shared::SolidBox {
                            center: Vec3::ZERO,
                            half_size: Vec3::splat(0.05),
                        }
                        .into(),
                    ],
                })
                .into(),
        };
        assert_eq!(summary(&snapshot, scenario.structure).fragments, 2);
        let target = next_target(&scenario, &snapshot, 1).unwrap();
        let candidate = barrier_cells()
            .into_iter()
            .find(|cell| {
                cell.structure == scenario.structure
                    && (target - Vec3::from(cell.center))
                        .abs()
                        .cmple(Vec3::from(cell.half_size) + Vec3::splat(1e-5))
                        .all()
            })
            .unwrap();
        snapshot.cuts.push(shared::impact_cut(
            &candidate,
            target,
            Vec3::Z,
            WeaponKind::Rifle,
        ));
        let next = next_target(&scenario, &snapshot, 2).unwrap();
        assert!(
            target.distance(next) > 0.003,
            "next shot must leave the open wood bore"
        );
        assert_eq!(summary(&snapshot, scenario.structure).fragments, 2);
    }
}
