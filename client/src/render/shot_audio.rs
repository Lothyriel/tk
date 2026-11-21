//! Original procedural game effects; no recordings or external assets.
use crate::{ClientSystems, sync::ClientSession};
use bevy::{
    audio::{AddAudioSource, ChannelCount, Decodable, SampleRate, Source, Volume},
    prelude::*,
};
use common::{
    PlayerId, WeaponInstanceId, WeaponKind,
    catalog::{ManipulationKind, ManipulationPhase, MechanicalSnapshot},
    replication::shots::ShotWindow,
};
use std::{collections::HashMap, sync::Arc, time::Duration};

const RATE: u32 = 24_000;
const MAX_VOICES: usize = 12;

pub(super) struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        let playtest = std::env::args().any(|arg| arg == "--playtest");
        let probe = playtest && std::env::var_os("TK_PLAYTEST_AUDIO").is_some();
        let muted = (playtest && !probe) || std::env::var_os("TK_MUTE_AUDIO").is_some();
        app.add_audio_source::<SynthShot>()
            .insert_resource(Muted(muted))
            .add_systems(Startup, prepare)
            .add_systems(Update, present.in_set(ClientSystems::Present));
        if probe {
            app.add_systems(Update, report_playback);
        }
    }
}

#[derive(Resource)]
struct Muted(bool);

#[derive(Asset, TypePath)]
pub(crate) struct SynthShot(Arc<[f32]>);

pub(crate) struct Samples {
    data: Arc<[f32]>,
    cursor: usize,
}

impl Iterator for Samples {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = *self.data.get(self.cursor)?;
        self.cursor += 1;
        Some(sample)
    }
}

impl Source for Samples {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.data.len() - self.cursor)
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(1).unwrap()
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(RATE).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(
            self.data.len() as f64 / f64::from(RATE),
        ))
    }
}

impl Decodable for SynthShot {
    type Decoder = Samples;

    fn decoder(&self) -> Self::Decoder {
        Samples {
            data: self.0.clone(),
            cursor: 0,
        }
    }
}

fn synth(weapon: WeaponKind, softened: bool) -> SynthShot {
    let (tone, decay) = match weapon {
        WeaponKind::Rifle => (106.0, 34.0),
        WeaponKind::Pistol => (160.0, 46.0),
        WeaponKind::Smg => (132.0, 40.0),
    };
    let mut seed = 0x8bcc_1e79_u32;
    let mut low = 0.0;
    SynthShot(
        (0..RATE / 5)
            .map(|i| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 8) as f32 / 8_388_608.0 - 1.0;
                low += 0.18 * (noise - low);
                let t = i as f32 / RATE as f32;
                let attack = (t * 1_400.0).min(1.0);
                let tail = ((0.2 - t) * 100.0).clamp(0.0, 1.0);
                let body = (t * tone * std::f32::consts::TAU).sin();
                let transient = if softened {
                    0.18 * noise + 0.32 * low
                } else {
                    0.58 * noise
                };
                (transient * (-t * 135.0).exp() + (0.28 * body + 0.22 * low) * (-t * decay).exp())
                    * attack
                    * tail
            })
            .collect(),
    )
}

#[derive(Resource)]
struct Cues([Handle<SynthShot>; 14]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HandlingCue {
    Removed = 3,
    Inserted = 4,
    Fed = 5,
    Selector = 9,
    Equipped = 10,
    Lowering = 11,
    Inspect = 12,
    EmptyPress = 13,
}

fn handling_cue(before: MechanicalSnapshot, after: MechanicalSnapshot) -> Option<HandlingCue> {
    use ManipulationKind::{EmptyReload, Inspect, Unequip};
    use ManipulationPhase::{Equip, Feed, Finish, Insert, Remove};
    if before.dry_cue_sequence != after.dry_cue_sequence {
        return Some(HandlingCue::EmptyPress);
    }
    if after.action_id < before.action_id {
        return None;
    }
    if before.action_id != after.action_id {
        return (after.kind == Some(Unequip)).then_some(HandlingCue::Lowering);
    }
    if before.phase == Some(Equip) && !before.equipped && after.equipped && after.kind.is_none() {
        return Some(HandlingCue::Equipped);
    }
    if before.kind == Some(Inspect)
        && after.kind == before.kind
        && before.fraction < 0.30
        && (0.30..0.45).contains(&after.fraction)
    {
        return Some(HandlingCue::Inspect);
    }
    if before.mode != after.mode {
        return Some(HandlingCue::Selector);
    }
    if before.action_id != after.action_id
        || before.kind != after.kind
        || !after.reloading()
        || after.fraction <= before.fraction
    {
        return None;
    }
    match (before.phase?, after.phase?) {
        (Remove, Insert) => Some(HandlingCue::Removed),
        (Remove | Insert, Feed) => Some(HandlingCue::Inserted),
        (Remove | Insert | Feed, Finish) if after.kind == Some(EmptyReload) => {
            Some(HandlingCue::Fed)
        }
        (Remove | Insert, Finish) => Some(HandlingCue::Inserted),
        _ => None,
    }
}

fn synth_contact(tone: f32, decay: f32) -> SynthShot {
    SynthShot(
        (0..RATE / 16)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let envelope = (t * 4_000.0).min(1.0) * (-decay * t).exp();
                let phase = t * tone * std::f32::consts::TAU;
                0.5 * envelope * (phase.sin() + 0.4 * (phase * 2.37).sin())
            })
            .collect(),
    )
}

fn prepare(mut commands: Commands, mut assets: ResMut<Assets<SynthShot>>) {
    commands.insert_resource(Cues([
        assets.add(synth(WeaponKind::Rifle, false)),
        assets.add(synth(WeaponKind::Pistol, false)),
        assets.add(synth(WeaponKind::Smg, false)),
        assets.add(synth_contact(320.0, 180.0)),
        assets.add(synth_contact(510.0, 130.0)),
        assets.add(synth_contact(780.0, 105.0)),
        assets.add(synth(WeaponKind::Rifle, true)),
        assets.add(synth(WeaponKind::Pistol, true)),
        assets.add(synth(WeaponKind::Smg, true)),
        assets.add(synth_contact(920.0, 195.0)),
        assets.add(synth_contact(385.0, 125.0)),
        assets.add(synth_contact(255.0, 150.0)),
        assets.add(synth_contact(610.0, 210.0)),
        assets.add(synth_contact(1100.0, 240.0)),
    ]));
}

#[derive(Component)]
struct Voice(f32);

fn report_playback(
    local: Query<(), (With<Voice>, With<AudioSink>)>,
    remote: Query<(), (With<Voice>, With<SpatialAudioSink>)>,
    mut reported: Local<[bool; 2]>,
) {
    for (i, (kind, ready)) in [
        ("local", !local.is_empty()),
        ("spatial", !remote.is_empty()),
    ]
    .into_iter()
    .enumerate()
    {
        if ready && !reported[i] {
            info!("PLAYTEST AUDIO SINK {kind}");
            reported[i] = true;
        }
    }
}

#[derive(Default)]
struct History {
    raid: Option<u32>,
    shots: ShotWindow,
    reloads: HashMap<u64, (WeaponInstanceId, MechanicalSnapshot)>,
}

fn present(
    mut commands: Commands,
    time: Res<Time>,
    session: Res<ClientSession>,
    local: Res<PlayerId>,
    settings: (Res<Muted>, Res<Cues>),
    voices: Query<(Entity, &Voice)>,
    mut history: Local<History>,
) {
    let (muted, cues) = settings;
    let now = time.elapsed_secs();
    let raid = Some(session.raid.raid_id);
    let changed = history.raid != raid;
    if changed {
        *history = History { raid, ..default() };
    }
    let mut active = 0;
    for (entity, voice) in &voices {
        if changed || muted.0 || voice.0 <= now {
            commands.entity(entity).try_despawn();
        } else {
            active += 1;
        }
    }
    let mut play = |cue: &Handle<SynthShot>, position: Vec3, remote: bool, volume: f32| {
        if muted.0 || active >= MAX_VOICES || !position.is_finite() {
            return;
        }
        commands.spawn((
            AudioPlayer(cue.clone()),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(volume))
                .with_spatial(remote),
            Transform::from_translation(position),
            Voice(now + 0.4),
        ));
        active += 1;
    };
    for shot in &session.shots {
        if !history.shots.observe(shot.id) {
            continue;
        }
        let class = match shot.weapon.class() {
            WeaponKind::Rifle => 0,
            WeaponKind::Pistol => 1,
            WeaponKind::Smg => 2,
        };
        let cue = &cues.0[class + if shot.audio_signature < 100 { 6 } else { 0 }];
        let gain = (f32::from(shot.audio_signature) / 100.0).clamp(0.15, 1.25);
        let remote = shot.shooter != local.0;
        play(
            cue,
            Vec3::from(shot.muzzle),
            remote,
            gain * if remote { 0.45 } else { 0.25 },
        );
    }
    history
        .reloads
        .retain(|id, _| session.local(*id).is_some_and(|actor| actor.alive));
    for actor in session.players.iter().filter(|actor| actor.alive) {
        let previous = history
            .reloads
            .insert(actor.id, (actor.weapon_instance, actor.mechanics));
        if let Some((instance, before)) = previous
            && instance == actor.weapon_instance
            && let Some(cue) = handling_cue(before, actor.mechanics)
        {
            let remote = actor.id != local.0;
            play(
                &cues.0[cue as usize],
                Vec3::from(actor.pos),
                remote,
                if remote { 0.24 } else { 0.17 },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "opt-in CPU timing experiment; run without other workloads"]
    fn shot_audio_scheduling_cost() {
        const ACTORS: u64 = 32;
        const WARMUP: usize = 120;
        const FRAMES: usize = 1200;
        let run = |muted: bool| {
            let mut app = App::new();
            let session = ClientSession {
                players: (1..=ACTORS)
                    .map(|id| common::ClientData {
                        id,
                        alive: true,
                        pos: [id as f32, 1.0, 0.0],
                        ..default()
                    })
                    .collect(),
                ..default()
            };
            app.init_resource::<Time>()
                .insert_resource(session)
                .insert_resource(PlayerId(1))
                .insert_resource(Muted(muted))
                .insert_resource(Cues(std::array::from_fn(|_| Handle::default())))
                .add_systems(Update, present);
            let mut times = Vec::with_capacity(FRAMES);
            let mut peak_voices = 0;
            for frame in 0..WARMUP + FRAMES {
                app.world_mut()
                    .resource_mut::<Time>()
                    .advance_by(Duration::from_secs_f64(1.0 / 60.0));
                let mut session = app.world_mut().resource_mut::<ClientSession>();
                session.shots.clear();
                if frame % 6 == 0 {
                    session
                        .shots
                        .extend((1..=ACTORS).map(|shooter| common::FiredProjectileData {
                            id: frame as u64 * ACTORS + shooter,
                            shooter,
                            weapon: common::WeaponId::M4A1,
                            time: frame as f32 / 60.0,
                            muzzle: [shooter as f32, 1.6, -1.0],
                            direction: [0.0, 0.0, -1.0],
                            ejection: None,
                            audio_signature: if shooter % 2 == 0 { 55 } else { 100 },
                        }));
                }
                let start = std::time::Instant::now();
                app.update();
                let elapsed = start.elapsed().as_secs_f64() * 1e6;
                if frame >= WARMUP {
                    times.push(elapsed);
                }
                let world = app.world_mut();
                peak_voices = peak_voices.max(world.query::<&Voice>().iter(world).count());
            }
            assert_eq!(peak_voices, if muted { 0 } else { MAX_VOICES });
            times.sort_by(f64::total_cmp);
            [50, 95, 99].map(|p| times[(FRAMES * p).div_ceil(100) - 1])
        };
        for repetition in 0..3 {
            for muted in [repetition % 2 == 0, repetition % 2 != 0] {
                let [p50, p95, p99] = run(muted);
                println!(
                    "TK AUDIO CPU run={} muted={muted} p50_us={p50:.2} p95_us={p95:.2} p99_us={p99:.2}",
                    repetition + 1
                );
            }
        }
    }

    #[test]
    fn shot_audio_reload_contacts_use_confirmed_phase_without_replaying_skipped_cues() {
        use ManipulationPhase::*;
        use common::catalog::ManipulationKind::{EmptyReload, TacticalReload};
        let state = |kind, phase, fraction| MechanicalSnapshot {
            action_id: 7,
            kind: Some(kind),
            phase: Some(phase),
            fraction,
            ..default()
        };
        let removed = state(EmptyReload, Insert, 0.30);
        let inserted = state(EmptyReload, Feed, 0.65);
        let finished = state(EmptyReload, Finish, 0.92);
        let started = state(EmptyReload, Remove, 0.15);
        assert_eq!(handling_cue(started, removed), Some(HandlingCue::Removed));
        assert_eq!(handling_cue(removed, inserted), Some(HandlingCue::Inserted));
        assert_eq!(handling_cue(inserted, finished), Some(HandlingCue::Fed));
        assert_eq!(handling_cue(started, finished), Some(HandlingCue::Fed));
        assert_eq!(handling_cue(finished, finished), None);
        assert_eq!(handling_cue(finished, inserted), None);
        assert_eq!(handling_cue(removed, MechanicalSnapshot::default()), None);
        assert_eq!(
            handling_cue(
                started,
                MechanicalSnapshot {
                    action_id: 8,
                    ..removed
                }
            ),
            None
        );
        assert_eq!(
            handling_cue(
                state(TacticalReload, Insert, 0.45),
                state(TacticalReload, Finish, 0.72)
            ),
            Some(HandlingCue::Inserted)
        );
    }

    #[test]
    fn shot_audio_handling_distinguishes_contacts_from_cancel_and_repeated_state() {
        let idle = MechanicalSnapshot::default();
        let clicked = MechanicalSnapshot {
            dry_cue_sequence: 1,
            ..idle
        };
        assert_eq!(handling_cue(idle, clicked), Some(HandlingCue::EmptyPress));
        assert_eq!(handling_cue(clicked, clicked), None);
        let lower = MechanicalSnapshot {
            action_id: 1,
            kind: Some(ManipulationKind::Unequip),
            phase: Some(ManipulationPhase::Unequip),
            ..idle
        };
        assert_eq!(handling_cue(idle, lower), Some(HandlingCue::Lowering));
        assert_eq!(handling_cue(lower, lower), None);
        let equip = MechanicalSnapshot {
            kind: Some(ManipulationKind::Equip),
            phase: Some(ManipulationPhase::Equip),
            equipped: false,
            ..lower
        };
        let cancelled = MechanicalSnapshot {
            action_id: 1,
            equipped: false,
            ..idle
        };
        assert_eq!(handling_cue(equip, cancelled), None);
        assert_eq!(
            handling_cue(
                equip,
                MechanicalSnapshot {
                    equipped: true,
                    ..cancelled
                }
            ),
            Some(HandlingCue::Equipped)
        );
        let inspect = MechanicalSnapshot {
            action_id: 2,
            kind: Some(ManipulationKind::Inspect),
            phase: Some(ManipulationPhase::Inspect),
            fraction: 0.29,
            ..idle
        };
        let contact = MechanicalSnapshot {
            fraction: 0.31,
            ..inspect
        };
        assert_eq!(handling_cue(inspect, contact), Some(HandlingCue::Inspect));
        assert_eq!(handling_cue(contact, contact), None);
        assert_eq!(
            handling_cue(
                inspect,
                MechanicalSnapshot {
                    fraction: 0.8,
                    ..inspect
                }
            ),
            None
        );
        assert_eq!(
            handling_cue(
                idle,
                MechanicalSnapshot {
                    mode: common::catalog::FireMode::Semi,
                    ..idle
                }
            ),
            Some(HandlingCue::Selector)
        );
    }

    #[test]
    fn shot_audio_only_plays_fresh_events_with_bounded_voices_and_respects_mute() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<ClientSession>()
            .insert_resource(PlayerId(7))
            .insert_resource(Muted(false))
            .insert_resource(Cues(std::array::from_fn(|_| Handle::default())))
            .add_systems(Update, present);
        let count = |app: &mut App| {
            let world = app.world_mut();
            world.query::<&AudioPlayer<SynthShot>>().iter(world).count()
        };
        app.update();
        assert_eq!(count(&mut app), 0);
        let shot = |id| common::FiredProjectileData {
            id,
            shooter: 7,
            weapon: common::WeaponId::M4A1,
            time: 0.0,
            muzzle: [1.0, 2.0, 3.0],
            direction: [0.0, 0.0, -1.0],
            ejection: None,
            audio_signature: 100,
        };
        app.world_mut().resource_mut::<ClientSession>().shots = vec![shot(1), shot(1)];
        app.update();
        assert_eq!(count(&mut app), 1);
        app.update();
        assert_eq!(count(&mut app), 1);
        app.world_mut().resource_mut::<ClientSession>().shots = (2..30).map(shot).collect();
        app.update();
        assert_eq!(count(&mut app), MAX_VOICES);
        app.world_mut().resource_mut::<Muted>().0 = true;
        app.world_mut().resource_mut::<ClientSession>().shots = vec![shot(30)];
        app.update();
        assert_eq!(count(&mut app), 0);
        app.world_mut().resource_mut::<Muted>().0 = false;
        app.update();
        assert_eq!(count(&mut app), 0, "unmuting must not replay old shots");
    }

    #[test]
    fn shot_audio_sources_are_finite_distinct_and_fade_to_zero() {
        let sounds =
            [WeaponKind::Rifle, WeaponKind::Pistol, WeaponKind::Smg].map(|kind| synth(kind, false));
        for sound in &sounds {
            assert_eq!(sound.decoder().count(), RATE as usize / 5);
            assert!(sound.0.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
            assert!(sound.0.iter().any(|s| s.abs() > 0.05));
            assert_eq!(sound.0[0], 0.0);
            assert!(sound.0.last().unwrap().abs() < 0.001);
        }
        assert_ne!(sounds[0].0, sounds[1].0);
        assert_ne!(sounds[1].0, sounds[2].0);
        let softened = synth(WeaponKind::Rifle, true);
        assert_ne!(sounds[0].0, softened.0);
        assert!(softened.0.iter().any(|sample| sample.abs() > 0.05));
    }
}
