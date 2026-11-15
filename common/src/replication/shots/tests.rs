use super::*;
use crate::WeaponId;

fn shot(id: u64, time: f32) -> FiredProjectileData {
    FiredProjectileData {
        id,
        time,
        shooter: 7,
        weapon: WeaponId::M4A1,
        muzzle: [0.0; 3],
        direction: [0.0, 0.0, -1.0],
    }
}

fn bootstrap(receiver: &mut ShotReceiver, first_id: u64) {
    receiver
        .message(&data::encode(&ShotMessage::Bootstrap { first_id }))
        .unwrap();
}

#[test]
fn shots_deduplicate_reordered_channels_and_never_replay_prejoin_or_expired_events() {
    let mut receiver = ShotReceiver::default();
    assert!(receiver.snapshot(&[shot(99, 1.0)], 1.0, true).is_empty());
    bootstrap(&mut receiver, 100);
    assert!(receiver.snapshot(&[shot(99, 1.0)], 1.01, true).is_empty());
    let fast = receiver.snapshot(&[shot(102, 1.0)], 1.01, true);
    assert_eq!(fast[0].id, 102);
    let message = data::encode(&ShotMessage::Batch {
        raid: 1,
        time: 1.0,
        shots: vec![shot(100, 1.0), shot(101, 1.0), shot(102, 1.0)],
    });
    receiver.message(&message).unwrap();
    let delayed = receiver.drain(1, 1.02, true);
    assert_eq!(delayed.iter().map(|s| s.id).collect::<Vec<_>>(), [100, 101]);
    bootstrap(&mut receiver, 100); // repeated handshake must not erase dedup
    receiver.message(&message).unwrap();
    assert!(receiver.drain(1, 1.03, true).is_empty());
    assert!(receiver.snapshot(&delayed, 1.04, true).is_empty());
    receiver
        .message(&data::encode(&ShotMessage::Batch {
            raid: 1,
            time: 1.1,
            shots: vec![shot(103, 1.1)],
        }))
        .unwrap();
    assert!(receiver.drain(1, 2.0, true).is_empty());
    assert_eq!(receiver.stale, 1);
    assert!(receiver.snapshot(&[shot(103, 1.1)], 2.0, true).is_empty());
    assert_eq!(
        receiver.message(&data::encode(&ShotMessage::Bootstrap { first_id: 0 })),
        Err(ShotError::Bootstrap)
    );
}

#[test]
fn shots_wait_for_raid_and_clock_then_expire_without_reset_replay() {
    let mut receiver = ShotReceiver::default();
    bootstrap(&mut receiver, 0);
    receiver
        .message(&data::encode(&ShotMessage::Batch {
            raid: 2,
            time: 2.0,
            shots: vec![shot(1, 2.0)],
        }))
        .unwrap();
    assert!(receiver.drain(1, 2.0, true).is_empty());
    assert!(receiver.drain(2, 1.9, true).is_empty());
    assert_eq!(receiver.drain(2, 2.01, true)[0].id, 1);
    receiver
        .message(&data::encode(&ShotMessage::Batch {
            raid: 2,
            time: 2.05,
            shots: vec![shot(2, 2.05)],
        }))
        .unwrap();
    assert!(
        receiver.drain(2, 2.06, false).is_empty(),
        "lobby cannot present raid effects"
    );
    assert!(receiver.snapshot(&[shot(2, 2.05)], 2.07, true).is_empty());
    receiver
        .message(&data::encode(&ShotMessage::Batch {
            raid: 1,
            time: 1.0,
            shots: vec![shot(0, 1.0)],
        }))
        .unwrap();
    assert!(receiver.drain(2, 2.1, true).is_empty());
}

#[test]
fn shots_window_is_bounded_and_accepts_gaps_instead_of_high_water_only() {
    let mut window = ShotWindow::starting_at(100);
    assert!(!window.observe(99));
    assert!(window.observe(103));
    assert!(window.observe(101));
    assert!(window.observe(100));
    assert!(!window.observe(101));
    for id in 104..20_000 {
        assert!(window.observe(id));
    }
    assert!(window.seen.len() <= SHOT_WINDOW as usize);
    assert!(
        !window.observe(102),
        "expired gap cannot become a replay later"
    );
    assert!(window.observe(u64::MAX));
    assert!(!window.observe(0), "IDs must not wrap");
}

#[test]
fn shots_wire_and_queue_limits_fail_explicitly() {
    let mut receiver = ShotReceiver::default();
    let batch = ShotMessage::Batch {
        raid: 2,
        time: 1.0,
        shots: (0..MAX_SHOT_BATCH as u64).map(|id| shot(id, 1.0)).collect(),
    };
    let bytes = data::encode(&batch);
    println!(
        "TK SHOT BATCH count={} bytes={}",
        MAX_SHOT_BATCH,
        bytes.len()
    );
    assert!(bytes.len() < MAX_SHOT_MESSAGE_BYTES);
    assert_eq!(receiver.message(&bytes), Err(ShotError::Bootstrap));
    bootstrap(&mut receiver, 0);
    for _ in 0..MAX_PENDING_SHOTS / MAX_SHOT_BATCH {
        receiver.message(&bytes).unwrap();
    }
    assert_eq!(receiver.message(&bytes), Err(ShotError::Backpressure));
    assert_eq!(receiver.pending_count, MAX_PENDING_SHOTS);
    assert!(matches!(
        ShotMessage::decode(&vec![0; MAX_SHOT_MESSAGE_BYTES + 1]),
        Err(ShotError::PayloadSize)
    ));
    for shots in [
        vec![],
        vec![shot(1, 1.0); MAX_SHOT_BATCH + 1],
        vec![shot(2, 1.0), shot(1, 1.0)],
        vec![shot(1, f32::NAN)],
    ] {
        assert!(matches!(
            ShotMessage::decode(&data::encode(&ShotMessage::Batch {
                raid: 1,
                time: 1.0,
                shots
            })),
            Err(ShotError::Bounds)
        ));
    }
    // Releasing obsolete batches restores capacity without replaying them.
    assert!(receiver.drain(3, 3.0, true).is_empty());
    assert_eq!(receiver.pending_count, 0);
}
