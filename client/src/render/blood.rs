//! Cosmetic blood follows authoritative anatomy; it never applies damage.
use crate::{ClientSystems, sync::ClientSession};
use bevy::{platform::collections::HashMap, prelude::*};
use common::{ClientData, combat::Region, humanoid::*, raid::RaidPhase};
use std::collections::VecDeque;

const MAX_DROPS: usize = 512;
const MAX_STAINS: usize = 256;
const DROP_SECONDS: f32 = 3.0;
const STAIN_SECONDS: f32 = 60.0;
const PIECE_SECONDS: f32 = 6.0;
const FLOOR: f32 = 0.106;

pub struct Plugin;
impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BloodState>()
            .add_systems(Startup, setup)
            .add_systems(Update, update.in_set(ClientSystems::Present));
    }
}

#[derive(Resource)]
struct BloodAssets {
    drop_mesh: Handle<Mesh>,
    stain_mesh: Handle<Mesh>,
    wet: Handle<StandardMaterial>,
    stain: Handle<StandardMaterial>,
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(BloodAssets {
        drop_mesh: meshes.add(Cuboid::new(0.014, 0.032, 0.014)),
        stain_mesh: meshes.add(Cylinder::new(1.0, 1.0)),
        wet: materials.add(StandardMaterial {
            base_color: Color::srgb(0.46, 0.006, 0.009),
            perceptual_roughness: 0.22,
            ..default()
        }),
        stain: materials.add(StandardMaterial {
            base_color: Color::srgb(0.23, 0.004, 0.006),
            perceptual_roughness: 0.65,
            ..default()
        }),
    });
}

#[derive(Default)]
struct ActorHistory {
    trauma: [f32; SEGMENT_COUNT],
    wounds: u32,
    missing: u32,
    drip: f32,
    cursor: usize,
}
#[derive(Default)]
struct PieceHistory {
    age: f32,
    drip: f32,
}

#[derive(Default, Resource)]
struct BloodState {
    raid: Option<u32>,
    phase: Option<RaidPhase>,
    sequence: u64,
    actors: HashMap<u64, ActorHistory>,
    pieces: HashMap<u64, PieceHistory>,
    stains: VecDeque<Entity>,
    seed: u32,
}

#[derive(Clone, Copy, Debug)]
struct Emission {
    position: Vec3,
    count: usize,
    burst: bool,
}

fn source(player: &ClientData, segment: Segment) -> Vec3 {
    let rig = evaluate_pose(
        &player.pose,
        player.body.anatomy.capacity(Region::LeftLeg),
        player.body.anatomy.capacity(Region::RightLeg),
    );
    let part = rig[segment.index()].transform();
    let root = Transform::from_translation(player.pos.into())
        .with_rotation(Quat::from_rotation_y(player.rot.yaw));
    root.transform_point(part.transform_point(Vec3::new(0.0, 0.0, -segment.half_size().z)))
}

fn wounded_sources(player: &ClientData) -> Vec<Segment> {
    SEGMENTS
        .into_iter()
        .filter(|segment| {
            player.body.anatomy.wounds & (1 << segment.index()) != 0
                && player.body.anatomy.missing & (1 << segment.index()) == 0
        })
        .collect()
}

impl BloodState {
    fn reset_needed(&mut self, session: &ClientSession) -> bool {
        let reset = self.raid != Some(session.raid.raid_id)
            || self.phase != Some(session.raid.phase) && session.raid.phase == RaidPhase::Lobby;
        if reset {
            self.actors.clear();
            self.pieces.clear();
            self.sequence = 0;
        }
        self.raid = Some(session.raid.raid_id);
        self.phase = Some(session.raid.phase);
        reset
    }

    fn emissions(&mut self, session: &ClientSession, delta: f32, fresh: bool) -> Vec<Emission> {
        let mut result = Vec::new();
        let new_snapshot = session.last_sequence > self.sequence;
        let established = self.sequence > 0;
        for player in &session.players {
            let anatomy = &player.body.anatomy;
            let previous = self
                .actors
                .entry(player.id)
                .or_insert_with(|| ActorHistory {
                    trauma: anatomy.trauma,
                    wounds: anatomy.wounds,
                    missing: anatomy.missing,
                    ..default()
                });
            if new_snapshot {
                for segment in SEGMENTS {
                    let bit = 1 << segment.index();
                    let new_missing = anatomy.missing & !previous.missing & bit != 0;
                    let missing_parent = segment
                        .parent()
                        .is_some_and(|p| anatomy.missing & (1 << p.index()) != 0);
                    if new_missing && !missing_parent {
                        result.push(Emission {
                            position: source(player, segment.parent().unwrap_or(segment)),
                            count: 24,
                            burst: true,
                        });
                    } else if anatomy.missing & bit == 0
                        && (anatomy.trauma[segment.index()]
                            > previous.trauma[segment.index()] + 0.01
                            || anatomy.wounds & !previous.wounds & bit != 0)
                    {
                        result.push(Emission {
                            position: source(player, segment),
                            count: 8,
                            burst: true,
                        });
                    }
                }
                previous.trauma = anatomy.trauma;
                previous.wounds = anatomy.wounds;
                previous.missing = anatomy.missing;
            }
            let sources = wounded_sources(player);
            if fresh
                && session.raid.phase == RaidPhase::Active
                && player.alive
                && anatomy.blood > 0.0
                && anatomy.bleed_rate() > 0.0
                && !sources.is_empty()
            {
                previous.drip += delta * (2.0 + anatomy.bleed_rate() * 2.0).min(12.0);
                while previous.drip >= 1.0 {
                    let segment = sources[previous.cursor % sources.len()];
                    previous.cursor = previous.cursor.wrapping_add(1);
                    result.push(Emission {
                        position: source(player, segment),
                        count: 1,
                        burst: false,
                    });
                    previous.drip -= 1.0;
                }
            } else {
                previous.drip = 0.0;
            }
        }
        self.actors
            .retain(|id, _| session.players.iter().any(|p| p.id == *id));
        for piece in session.physics_pieces.iter().filter(|p| p.detached) {
            let history = self.pieces.entry(piece.id).or_insert(PieceHistory {
                age: if established { 0.0 } else { PIECE_SECONDS },
                drip: 0.0,
            });
            history.age += delta;
            if fresh && history.age < PIECE_SECONDS && session.raid.phase == RaidPhase::Active {
                history.drip += delta * 4.0;
                while history.drip >= 1.0 {
                    result.push(Emission {
                        position: piece.pose.position.into(),
                        count: 1,
                        burst: false,
                    });
                    history.drip -= 1.0;
                }
            }
        }
        self.pieces
            .retain(|id, _| session.physics_pieces.iter().any(|p| p.id == *id));
        self.sequence = self.sequence.max(session.last_sequence);
        result
    }

    fn random(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        self.seed as f32 / u32::MAX as f32
    }
}

#[derive(Component)]
struct Blood {
    velocity: Vec3,
    seconds: f32,
    stain: bool,
}

/// Returns true only on the first contact with the yard floor.
fn advance(blood: &mut Blood, transform: &mut Transform, delta: f32) -> bool {
    blood.seconds -= delta;
    if blood.stain || blood.seconds <= 0.0 {
        return false;
    }
    blood.velocity.y -= 9.81 * delta;
    transform.translation += blood.velocity * delta;
    if transform.translation.y <= FLOOR {
        transform.translation.y = FLOOR;
        blood.stain = true;
        blood.seconds = STAIN_SECONDS;
        return true;
    }
    false
}

fn update(
    mut commands: Commands,
    time: Res<Time>,
    real: Res<Time<Real>>,
    session: Res<ClientSession>,
    assets: Res<BloodAssets>,
    mut state: ResMut<BloodState>,
    mut effects: Query<(
        Entity,
        &mut Blood,
        &mut Transform,
        &mut Mesh3d,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    if state.reset_needed(&session) {
        for (entity, ..) in &effects {
            commands.entity(entity).despawn();
        }
        state.stains.clear();
        return;
    }
    let delta = time.delta_secs().min(0.05);
    let mut drops = 0;
    for (entity, mut blood, mut transform, mut mesh, mut material) in &mut effects {
        if advance(&mut blood, &mut transform, delta) {
            if state.stains.len() == MAX_STAINS
                && let Some(old) = state.stains.pop_front()
            {
                commands.entity(old).despawn();
            }
            state.stains.push_back(entity);
            mesh.0 = assets.stain_mesh.clone();
            material.0 = assets.stain.clone();
            let radius = 0.035 + state.random() * 0.075;
            transform.scale = Vec3::new(radius, 0.002, radius * (0.6 + state.random()));
        }
        if blood.seconds <= 0.0 {
            commands.entity(entity).despawn();
            state.stains.retain(|e| *e != entity);
        } else if !blood.stain {
            drops += 1;
        }
    }
    let fresh = real.elapsed_secs_f64() - session.last_received <= 0.5;
    for emission in state.emissions(&session, delta, fresh) {
        if !emission.position.is_finite() {
            continue;
        }
        for _ in 0..emission.count.min(MAX_DROPS - drops) {
            let velocity = if emission.burst {
                Vec3::new(
                    (state.random() - 0.5) * 4.0,
                    0.5 + state.random() * 2.5,
                    (state.random() - 0.5) * 4.0,
                )
            } else {
                Vec3::new(
                    (state.random() - 0.5) * 0.15,
                    -0.3,
                    (state.random() - 0.5) * 0.15,
                )
            };
            commands.spawn((
                Blood {
                    velocity,
                    seconds: DROP_SECONDS,
                    stain: false,
                },
                Mesh3d(assets.drop_mesh.clone()),
                MeshMaterial3d(assets.wet.clone()),
                Transform::from_translation(emission.position),
            ));
            drops += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{ClientData, PhysicsPieceData, humanoid::SegmentPose};

    fn session() -> ClientSession {
        let mut session = ClientSession::default();
        session.raid.phase = RaidPhase::Active;
        session.raid.raid_id = 1;
        session.last_sequence = 1;
        session.players.push(ClientData {
            id: 7,
            alive: true,
            pos: [0.0, 1.05, 0.0],
            ..default()
        });
        session
    }

    #[test]
    fn blood_bursts_follow_new_trauma_and_separation_without_snapshot_replay() {
        let mut state = BloodState::default();
        let mut session = session();
        assert!(state.emissions(&session, 0.0, true).is_empty());
        let anatomy = &mut session.players[0].body.anatomy;
        anatomy.trauma[Segment::LeftForearm.index()] = 26.0;
        anatomy.wounds |= 1 << Segment::LeftForearm.index();
        session.last_sequence += 1;
        let bursts = state.emissions(&session, 0.0, true);
        assert_eq!(bursts.len(), 1);
        assert_eq!(bursts[0].count, 8);
        assert_eq!(
            bursts[0].position,
            source(&session.players[0], Segment::LeftForearm)
        );
        assert!(state.emissions(&session, 0.0, true).is_empty());
        session.last_sequence += 1;
        assert!(state.emissions(&session, 0.0, true).is_empty());
        session.players[0].body.anatomy.trauma[Segment::LeftForearm.index()] += 26.0;
        session.last_sequence += 1;
        assert_eq!(
            state.emissions(&session, 0.0, true)[0].count,
            8,
            "repeated hit to the same wound must spray"
        );
        session.players[0]
            .body
            .anatomy
            .separate(Segment::LeftForearm);
        session.last_sequence += 1;
        let bursts = state.emissions(&session, 0.0, true);
        let stumps: Vec<_> = bursts.iter().filter(|e| e.count == 24).collect();
        assert_eq!(
            stumps.len(),
            1,
            "descendants must not duplicate severance bursts"
        );
        assert_eq!(
            stumps[0].position,
            source(&session.players[0], Segment::LeftUpperArm)
        );
        let mut late = BloodState::default();
        assert!(
            late.emissions(&session, 0.0, true).is_empty(),
            "late arrival must not replay injury bursts"
        );
    }

    #[test]
    fn blood_burst_origin_matches_a_known_rotated_world_segment() {
        let mut session = session();
        let player = &mut session.players[0];
        player.pos = [10.0, 2.0, 3.0];
        player.rot.yaw = std::f32::consts::FRAC_PI_2;
        let mut rig = [SegmentPose::default(); SEGMENT_COUNT];
        rig[Segment::LeftForearm.index()].position = [1.0, 2.0, 3.0];
        player.pose.physical = Some(Box::new(PackedRig::pack(&rig)));
        let mut state = BloodState::default();
        state.emissions(&session, 0.0, true);
        session.players[0].body.anatomy.trauma[Segment::LeftForearm.index()] = 26.0;
        session.last_sequence += 1;
        let bursts = state.emissions(&session, 0.0, true);
        assert_eq!(bursts.len(), 1);
        // Forearm front is 0.075 m before local Z=3. A +90 degree root
        // rotation maps that point to (2.925, 2, -1), then adds the root.
        assert!(
            bursts[0].position.distance(Vec3::new(12.925, 4.0, 2.0)) < 0.001,
            "blood burst must follow the actual world segment: {:?}",
            bursts[0].position
        );
    }

    #[test]
    fn blood_drips_for_all_bleeding_actor_types_and_stops_when_stale_or_dead() {
        for bot in [false, true] {
            let mut state = BloodState::default();
            let mut session = session();
            session.players[0].bot = bot;
            session.players[0].body.anatomy.wounds = 1 << Segment::Thorax.index();
            session.players[0].body.anatomy.bleeding = 1.0;
            state.emissions(&session, 0.0, true);
            let drops = state.emissions(&session, 1.0, true);
            assert_eq!(drops.len(), 4);
            assert!(drops.iter().all(|d| d.count == 1
                && !d.burst
                && d.position == source(&session.players[0], Segment::Thorax)));
            assert!(state.emissions(&session, 1.0, false).is_empty());
            session.players[0].alive = false;
            assert!(state.emissions(&session, 1.0, true).is_empty());
            session.players[0].alive = true;
            session.players[0].body.anatomy.bleeding = 0.0;
            assert!(state.emissions(&session, 1.0, true).is_empty());
            session.players[0].body.anatomy.bleeding = 100.0;
            assert_eq!(state.emissions(&session, 1.0, true).len(), 12);
            session.players[0].body.anatomy.blood = 0.0;
            assert!(state.emissions(&session, 1.0, true).is_empty());
        }
    }

    #[test]
    fn blood_detached_pieces_drip_for_six_seconds_without_late_join_replay() {
        let mut state = BloodState::default();
        let mut session = session();
        state.emissions(&session, 0.0, true);
        session.physics_pieces.push(PhysicsPieceData {
            id: 44,
            owner: 7,
            segment: Segment::LeftForearm,
            pose: SegmentPose {
                position: [4.0, 0.2, 3.0],
                ..default()
            },
            detached: true,
            bot: false,
        });
        session.last_sequence += 1;
        let drips = state.emissions(&session, 1.0, true);
        assert_eq!(drips.len(), 4);
        assert!(
            drips
                .iter()
                .all(|e| e.position == Vec3::new(4.0, 0.2, 3.0) && !e.burst)
        );
        assert!(state.emissions(&session, PIECE_SECONDS, true).is_empty());
        let mut late = BloodState::default();
        assert!(late.emissions(&session, 1.0, true).is_empty());
    }

    #[test]
    fn blood_floor_contact_and_raid_reset_preserve_finite_lifetimes() {
        let mut blood = Blood {
            velocity: -Vec3::Y,
            seconds: DROP_SECONDS,
            stain: false,
        };
        let mut transform = Transform::from_xyz(0.0, FLOOR + 0.01, 0.0);
        assert!(advance(&mut blood, &mut transform, 0.1));
        assert!(blood.stain);
        assert_eq!(blood.seconds, 60.0);
        assert_eq!(transform.translation.y, FLOOR);
        assert!(!advance(&mut blood, &mut transform, 60.0));
        assert_eq!(blood.seconds, 0.0);
        let mut expired = Blood {
            velocity: -Vec3::Y,
            seconds: 0.01,
            stain: false,
        };
        let mut airborne = Transform::from_xyz(0.0, 3.0, 0.0);
        assert!(!advance(&mut expired, &mut airborne, 0.1));
        assert!(!expired.stain);
        assert!(expired.seconds < 0.0);
        let mut expired_near_floor = Blood {
            velocity: -Vec3::Y,
            seconds: 0.01,
            stain: false,
        };
        assert!(!advance(&mut expired_near_floor, &mut transform, 0.1));
        assert!(
            !expired_near_floor.stain,
            "expired droplets must not restart life as a stain"
        );
        let mut full_lifetime = Blood {
            velocity: Vec3::ZERO,
            seconds: DROP_SECONDS,
            stain: false,
        };
        let mut high = Transform::from_xyz(0.0, 100.0, 0.0);
        assert!(!advance(&mut full_lifetime, &mut high, 3.0));
        assert!(full_lifetime.seconds <= 0.0);
        assert!(!full_lifetime.stain);
        let mut state = BloodState::default();
        let mut session = session();
        state.reset_needed(&session);
        state.emissions(&session, 0.0, true);
        assert_eq!(state.actors.len(), 1);
        session.raid.phase = RaidPhase::Lobby;
        assert!(state.reset_needed(&session));
        assert!(state.actors.is_empty());
        assert!(state.pieces.is_empty());
        session.raid.raid_id += 1;
        assert!(state.reset_needed(&session));
    }

    #[test]
    fn blood_ecs_caps_drop_and_stain_counts_and_clears_on_lobby() {
        use bevy::time::TimeUpdateStrategy;
        use std::time::Duration;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                0.05,
            )))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .insert_resource(session())
            .add_plugins(Plugin);
        app.update();
        app.update();
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.players = (0..32)
                .map(|id| ClientData {
                    id,
                    alive: true,
                    pos: [id as f32 * 0.1, 1.05, 0.0],
                    ..default()
                })
                .collect();
            session.last_sequence += 1;
        }
        app.update();
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            for p in &mut session.players {
                p.body.anatomy.trauma = [10.0; SEGMENT_COUNT];
                p.body.anatomy.wounds = (1 << SEGMENT_COUNT) - 1;
            }
            session.last_sequence += 1;
        }
        app.update();
        let count = |app: &mut App, stain: bool| {
            app.world_mut()
                .query::<&Blood>()
                .iter(app.world())
                .filter(|b| b.stain == stain)
                .count()
        };
        assert_eq!(count(&mut app, false), 512);
        for _ in 0..65 {
            app.update();
        }
        assert_eq!(count(&mut app, false), 0);
        assert_eq!(count(&mut app, true), 256);
        app.world_mut().resource_mut::<ClientSession>().raid.phase = RaidPhase::Lobby;
        app.update();
        assert_eq!(count(&mut app, true), 0);
        assert!(app.world().resource::<BloodState>().actors.is_empty());
    }
}
