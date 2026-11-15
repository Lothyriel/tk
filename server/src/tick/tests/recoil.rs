use super::*;
use common::humanoid::PoseState;
use std::collections::BTreeMap;

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
