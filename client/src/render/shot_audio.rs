//! Original procedural game effects; no recordings or external assets.
use crate::{ClientSystems, sync::ClientSession};
use bevy::{
    audio::{AddAudioSource, ChannelCount, Decodable, SampleRate, Source, Volume},
    prelude::*,
};
use common::{PlayerId, WeaponKind, replication::shots::ShotWindow};
use std::{sync::Arc, time::Duration};

const RATE: u32 = 24_000;
const MAX_VOICES: usize = 12;

pub(super) struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        let muted = std::env::args().any(|arg| arg == "--playtest")
            || std::env::var_os("TK_MUTE_AUDIO").is_some();
        app.add_audio_source::<SynthShot>()
            .insert_resource(Muted(muted))
            .add_systems(Startup, prepare)
            .add_systems(Update, present.in_set(ClientSystems::Present));
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

fn synth(weapon: WeaponKind) -> SynthShot {
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
                (0.58 * noise * (-t * 135.0).exp()
                    + (0.28 * body + 0.22 * low) * (-t * decay).exp())
                    * attack
                    * tail
            })
            .collect(),
    )
}

#[derive(Resource)]
struct Cues([Handle<SynthShot>; 3]);

fn prepare(mut commands: Commands, mut assets: ResMut<Assets<SynthShot>>) {
    commands.insert_resource(Cues(
        [WeaponKind::Rifle, WeaponKind::Pistol, WeaponKind::Smg]
            .map(|kind| assets.add(synth(kind))),
    ));
}

#[derive(Component)]
struct Voice(f32);

fn present(
    mut commands: Commands,
    time: Res<Time>,
    session: Res<ClientSession>,
    local: Res<PlayerId>,
    settings: (Res<Muted>, Res<Cues>),
    voices: Query<(Entity, &Voice)>,
    mut history: Local<(Option<u32>, ShotWindow)>,
) {
    let (muted, cues) = settings;
    let now = time.elapsed_secs();
    let raid = Some(session.raid.raid_id);
    let changed = history.0 != raid;
    if changed {
        *history = (raid, ShotWindow::default());
    }
    let mut active = 0;
    for (entity, voice) in &voices {
        if changed || muted.0 || voice.0 <= now {
            commands.entity(entity).try_despawn();
        } else {
            active += 1;
        }
    }
    for shot in &session.shots {
        if !history.1.observe(shot.id) || muted.0 || active >= MAX_VOICES {
            continue;
        }
        let position = Vec3::from(shot.muzzle);
        if !position.is_finite() {
            continue;
        }
        let cue = match shot.weapon.class() {
            WeaponKind::Rifle => &cues.0[0],
            WeaponKind::Pistol => &cues.0[1],
            WeaponKind::Smg => &cues.0[2],
        };
        let remote = shot.shooter != local.0;
        commands.spawn((
            AudioPlayer(cue.clone()),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(if remote { 0.45 } else { 0.25 }))
                .with_spatial(remote),
            Transform::from_translation(position),
            Voice(now + 0.4),
        ));
        active += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let sounds = [WeaponKind::Rifle, WeaponKind::Pistol, WeaponKind::Smg].map(synth);
        for sound in &sounds {
            assert_eq!(sound.decoder().count(), RATE as usize / 5);
            assert!(sound.0.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
            assert!(sound.0.iter().any(|s| s.abs() > 0.05));
            assert_eq!(sound.0[0], 0.0);
            assert!(sound.0.last().unwrap().abs() < 0.001);
        }
        assert_ne!(sounds[0].0, sounds[1].0);
        assert_ne!(sounds[1].0, sounds[2].0);
    }
}
