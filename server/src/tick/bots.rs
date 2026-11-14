use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use common::{combat::BodyHealth, raid::*, *};

#[derive(Debug, Component)]
pub(super) struct Bot {
    pub next_shot: f32,
}

pub(super) fn bot_tick(
    time: Res<Time>,
    raid: Res<RaidState>,
    context: ReadRapierContext,
    targets: Query<(Entity, &Transform, &BodyHealth, &Participant), Without<Bot>>,
    mut bots: Query<(Entity, &Transform, &BodyHealth, &mut ClientInput, &mut Bot), With<Bot>>,
) {
    let Ok(context) = context.single() else {
        return;
    };
    for (entity, transform, body, mut input, mut bot) in &mut bots {
        input.fire = false;
        input.weapon = WeaponKind::Pistol;
        if raid.phase != RaidPhase::Active || !body.alive() {
            continue;
        }
        let target = targets
            .iter()
            .filter(|(_, _, body, p)| body.alive() && p.status == RaidStatus::Active)
            .filter(|(_, t, _, _)| t.translation.distance(transform.translation) < 18.0)
            .min_by(|a, b| {
                a.1.translation
                    .distance_squared(transform.translation)
                    .total_cmp(&b.1.translation.distance_squared(transform.translation))
            });
        let Some((target_entity, target_transform, ..)) = target else {
            continue;
        };
        let origin = transform.translation + Vec3::Y * PLAYER_VIEW_HEIGHT;
        let displacement = target_transform.translation - origin;
        let distance = displacement.length();
        if distance < 0.01 {
            continue;
        }
        let direction = displacement / distance;
        let visible = context
            .cast_ray(
                origin,
                direction,
                distance + 0.1,
                true,
                QueryFilter::new()
                    .exclude_collider(entity)
                    .exclude_sensors(),
            )
            .is_some_and(|(hit, _)| hit == target_entity);
        if !visible {
            continue;
        }
        input.camera = Quat::from_rotation_arc(-Vec3::Z, direction).into();
        if time.elapsed_secs() >= bot.next_shot {
            input.fire_pressed_sequence = input.fire_pressed_sequence.wrapping_add(1);
            bot.next_shot = time.elapsed_secs() + 1.5;
        }
        // Reloads remain subject to the same finite reserve and timer as human players.
        input.reload_sequence = (time.elapsed_secs() / 28.0) as u32;
    }
}
