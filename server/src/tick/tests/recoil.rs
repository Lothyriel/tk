use super::*;
use common::humanoid::PoseState;
use std::collections::BTreeMap;

#[test]
fn recoil_m4a1_kits_keep_cadence_but_change_confirmed_kick_and_recovery() {
    use bevy_renet2::prelude::RenetClient;
    use common::catalog::AnchorKind;

    let mut results = Vec::new();
    for kit in ["compact", "suppressed"] {
        let (mut app, shooter, other) = physical_fixture();
        app.world_mut()
            .get_mut::<Transform>(other)
            .unwrap()
            .translation
            .x = -14.0;
        let arsenal = crate::kit_fixture::KitFixture::named(kit).arsenal(Loadout::default());
        let start_rounds = arsenal.current().rounds();
        let assembly = &arsenal.current().spec().assembly;
        let outlet = assembly
            .hip_from_root
            .transform_point(assembly.anchors[&AnchorKind::DeviceExit].translation);
        *app.world_mut().get_mut::<Arsenal>(shooter).unwrap() = arsenal;

        // Production broadcast, in-memory Renet only: drop all unreliable
        // snapshots until the complete burst can be confirmed in one update.
        let mut peer = RenetClient::new(data::renet_config(), false);
        peer.set_connected();
        app.world_mut()
            .resource_mut::<RenetServer>()
            .add_connection(999, false);
        let mut shots = BTreeMap::new();
        let mut confirmation = false;
        let mut dropped = 0;
        let mut recovery = Vec::new();
        for _ in 0..128 {
            let mut input = app.world_mut().get_mut::<ClientInput>(shooter).unwrap();
            input.fire = shots.len() < 3;
            input.fire_pressed_sequence = 1;
            held_steps(&mut app, shooter, 1);

            let world = app.world();
            let recoil = world.get::<Arsenal>(shooter).unwrap().recoil;
            let pose = world.get::<PoseState>(shooter).unwrap();
            let root = world.get::<Transform>(shooter).unwrap();
            let eye = root.translation + Vec3::Y * PLAYER_VIEW_HEIGHT;
            let aim = Quat::from_euler(EulerRot::YXZ, recoil.angles[0], pose.aim_pitch, 0.0);
            for shot in &world.resource::<WorldState>().fired_projectiles {
                if shots.contains_key(&shot.id) {
                    continue;
                }
                // Injection on this tick changes velocity, not the angles
                // used by the projectile that caused it.
                assert!(Vec3::from(shot.direction).distance(aim * Vec3::NEG_Z) < 1e-6);
                assert!(Vec3::from(shot.muzzle).distance(eye + aim * outlet) < 0.001);
                shots.insert(shot.id, (shot.time, Vec3::from(shot.direction)));
            }
            assert_eq!(recoil.impulses as usize, shots.len());
            if shots.len() == 3 {
                recovery.push(recoil.angles[1]);
            }

            for packet in app
                .world_mut()
                .resource_mut::<RenetServer>()
                .get_packets_to_send(999)
                .unwrap()
            {
                peer.process_packet(packet);
            }
            while let Some(bytes) = peer.receive_message(DefaultChannel::Unreliable) {
                if shots.len() < 3 {
                    dropped += 1;
                    continue;
                }
                let snapshot: WorldSnapshot = data::decode(&bytes);
                let client = snapshot.players.iter().find(|p| p.id == 7).unwrap();
                let confirmed = client.recoil.unpack();
                assert_ne!(client.configuration, 0);
                assert_eq!(client.ammo_in_mag, start_rounds - 3);
                for axis in 0..2 {
                    assert!((confirmed.angles[axis] - recoil.angles[axis]).abs() <= 1.0 / 65536.0);
                    assert!(
                        (confirmed.velocity[axis] - recoil.velocity[axis]).abs() <= 1.0 / 8192.0
                    );
                }
                if !confirmation {
                    assert!(confirmed.angles[1] > 0.01);
                    assert_eq!(
                        snapshot
                            .fired_projectiles
                            .iter()
                            .map(|s| s.id)
                            .collect::<Vec<_>>(),
                        shots.keys().copied().collect::<Vec<_>>(),
                    );
                    confirmation = true;
                }
            }
        }
        assert!(
            dropped > 0 && confirmation,
            "{kit}: missing loss/confirmation"
        );
        let shots: Vec<_> = shots.into_values().collect();
        assert_eq!(shots.len(), 3);
        assert_eq!(shots[0].1, Vec3::NEG_Z);
        assert!(shots[1].1.x.abs() > 0.0001);
        let peak = recovery.iter().copied().fold(0.0_f32, f32::max);
        assert!((0.01..0.12).contains(&peak));
        assert!(
            recovery[64] < 0.003,
            "{kit}: failed to recover after half a second"
        );
        let tail = recovery[40] / peak;
        eprintln!(
            "{kit}: second-shot direction={:?}, peak={peak:.6} rad, \
             remaining/peak after 40 ticks={tail:.6}, dropped snapshots={dropped}",
            shots[1].1
        );
        results.push((shots, peak, tail));
    }

    let (compact, compact_peak, compact_tail) = &results[0];
    let (suppressed, suppressed_peak, suppressed_tail) = &results[1];
    for (a, b) in compact.iter().zip(suppressed) {
        assert_eq!(a.0, b.0, "kit must not change cadence");
    }
    assert!(
        compact[1].1.y > suppressed[1].1.y + 0.002
            && compact[1].1.x.abs() > suppressed[1].1.x.abs()
            && compact_peak > suppressed_peak,
        "expected stronger compact kick: compact={compact:?} peak={compact_peak}, \
         suppressed={suppressed:?} peak={suppressed_peak}"
    );
    assert!(
        suppressed_tail > compact_tail,
        "expected slower normalized recovery for suppressed: {suppressed_tail} vs {compact_tail}"
    );
}

#[test]
fn recoil_authority_is_identical_for_30_60_165_fps_with_loss_and_duplicate_input() {
    let mut reference = None;
    for hz in [30, 60, 165] {
        let (mut app, player, _) = physical_fixture();
        let mut shots = BTreeMap::new();
        let mut next_packet = 0.0;
        let mut packet = 0;
        for tick in 0..512 {
            let now = tick as f64 / 128.0;
            if now >= next_packet {
                next_packet += 1.0 / hz as f64;
                packet += 1;
                // Simulated client packet production: same held voluntary aim,
                // repeating edge sequence, every third packet dropped.
                if packet % 3 != 0 {
                    let time = app.world().resource::<Time>().elapsed_secs();
                    app.world_mut()
                        .get_mut::<LastInput>(player)
                        .unwrap()
                        .received_at = time;
                    let mut input = app.world_mut().get_mut::<ClientInput>(player).unwrap();
                    input.fire = true;
                    input.fire_pressed_sequence = 1;
                    input.camera = CameraInput::default();
                }
            }
            app.update();
            for shot in &app.world().resource::<WorldState>().fired_projectiles {
                if shot.shooter == 7 {
                    shots.insert(shot.id, (shot.time, shot.direction));
                }
            }
            let arsenal = app.world().get::<Arsenal>(player).unwrap();
            assert_eq!(arsenal.recoil.impulses as usize, shots.len());
            let pose = app.world().get::<PoseState>(player).unwrap();
            assert!((pose.aim_pitch - arsenal.recoil.angles[1]).abs() < 1e-6);
        }
        assert_eq!(shots.len(), 30, "one impulse per real chambered round");
        let values: Vec<_> = shots.values().copied().collect();
        assert_eq!(
            values[0].1,
            [0.0, 0.0, -1.0],
            "first shot uses pre-impulse bore"
        );
        assert!(
            values[1].1[1] > 0.01,
            "subsequent shot rises without client aim change"
        );
        for pair in values.windows(2) {
            assert!((pair[1].0 - pair[0].0 - 13.0 / 128.0).abs() < 1e-6);
        }
        if let Some(expected) = &reference {
            assert_eq!(
                &values, expected,
                "client render/packet rate changed authority"
            );
        } else {
            reference = Some(values);
        }
        let arsenal = app.world().get::<Arsenal>(player).unwrap();
        assert_eq!(arsenal.current().rounds(), 0);
        assert!(
            arsenal.recoil.angles[1] < 0.0001,
            "dry held trigger must let recoil recover"
        );
        assert_eq!(
            app.world().get::<ClientInput>(player).unwrap().camera.pitch,
            0.0
        );
    }
}

#[test]
fn recoil_rejected_safe_and_repeated_semi_edges_have_no_impulse() {
    let (mut app, player, _) = physical_fixture();
    // Auto -> Safe; rejected held trigger cannot inject angular velocity.
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(player).unwrap();
        input.selector_sequence = 1;
        input.fire = true;
        input.fire_pressed_sequence = 1;
    }
    held_steps(&mut app, player, 64);
    assert_eq!(
        app.world().get::<Arsenal>(player).unwrap().recoil.impulses,
        0
    );
    // Safe -> Semi. A fresh press fires once despite duplicated input.
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(player).unwrap();
        input.selector_sequence = 2;
        input.fire_pressed_sequence = 2;
    }
    held_steps(&mut app, player, 64);
    assert_eq!(
        app.world().get::<Arsenal>(player).unwrap().recoil.impulses,
        1
    );
    app.world_mut()
        .get_mut::<ClientInput>(player)
        .unwrap()
        .fire_pressed_sequence = 3;
    held_steps(&mut app, player, 1);
    assert_eq!(
        app.world().get::<Arsenal>(player).unwrap().recoil.impulses,
        2
    );
}
