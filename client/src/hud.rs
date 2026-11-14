use bevy::prelude::*;
use bevy_renet2::prelude::RenetClient;
use common::{combat::*, map::*, raid::*, *};

use crate::{ClientSystems, sync::ClientSession};

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_systems(Update, update.in_set(ClientSystems::Present));
    }
}

#[derive(Component)]
struct RaidHud;

fn setup(mut commands: Commands) {
    commands.spawn((
        RaidHud,
        Text::new("TK / EXTRACTION ALPHA\nConnecting to server..."),
        TextFont::from_font_size(19.0),
        TextColor(Color::srgb(0.9, 0.97, 1.0)),
        BackgroundColor(Color::srgba(0.015, 0.025, 0.04, 0.88)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(18.0),
            left: Val::Px(18.0),
            max_width: Val::Px(680.0),
            padding: UiRect::all(Val::Px(18.0)),
            ..default()
        },
        GlobalZIndex(200),
    ));
}

fn update(
    time: Res<Time<Real>>,
    session: Res<ClientSession>,
    input: Res<LocalInput>,
    id: Res<PlayerId>,
    client: Res<RenetClient>,
    mut hud: Single<&mut Text, With<RaidHud>>,
) {
    let connection = if client.is_disconnected() {
        format!(
            "DISCONNECTED: {:?}\nRestart the client to reconnect.",
            client.disconnect_reason()
        )
    } else if !client.is_connected() || session.local(id.0).is_none() {
        "Connecting... Start the server with cargo run --bin server".to_string()
    } else if time.elapsed_secs_f64() - session.last_received > 2.0 {
        "CONNECTION STALE: waiting for authoritative state".to_string()
    } else {
        String::new()
    };
    ***hud = describe(&session, &input.loadout, id.0, &connection);
}

fn describe(session: &ClientSession, pending: &Loadout, id: u64, connection: &str) -> String {
    let mut text = format!(
        "TK / EXTRACTION ALPHA   |   RAID {}\n",
        session.raid.raid_id
    );
    if !connection.is_empty() {
        text.push_str(connection);
        text.push_str("\nEsc: release / capture cursor");
        return text;
    }
    let Some(local) = session.local(id) else {
        return text;
    };
    let p = &local.participant;
    match session.raid.phase {
        RaidPhase::Lobby | RaidPhase::Countdown => {
            text.push_str(if session.raid.phase == RaidPhase::Countdown {
                "DEPLOYING"
            } else {
                "LOBBY"
            });
            if session.raid.phase == RaidPhase::Countdown {
                text.push_str(&format!(" IN {:.1}s", session.raid.seconds_left));
            }
            let loadout = if p.ready { &p.loadout } else { pending };
            text.push_str(&format!(
                "\n\n[G] Primary: {}  +  Pistol\n[B] Ammo: {:?} (AP trades damage for penetration)\n[H] Vest: {:?}    [J] Helmet: {}\n\n[Enter] {}\n",
                loadout.primary.spec().name, loadout.ammo, loadout.armor,
                if loadout.helmet { "ON" } else { "OFF" },
                if p.ready { "READY - press again to cancel" } else { "Ready / deploy (solo supported)" },
            ));
            let mut roster: Vec<_> = session.players.iter().filter(|p| !p.bot).collect();
            roster.sort_by_key(|p| p.id);
            for player in roster {
                text.push_str(&format!(
                    "\n{} {:04x}   {}",
                    if player.id == id { "YOU" } else { "PMC" },
                    player.id & 0xffff,
                    if player.participant.ready {
                        "READY"
                    } else {
                        "NOT READY"
                    }
                ));
            }
            text.push_str(&format!("\n\nSecured this session: {}\nCollect orange crates. Return to the green exit.\nHold F inside the exit for 5s with loot.\nGear locks when ready. Esc: cursor.", p.secured));
        }
        RaidPhase::Active if p.status == RaidStatus::Active => {
            let body = &local.body;
            let nearest = LOOT_POSITIONS
                .iter()
                .enumerate()
                .filter(|(i, _)| session.raid.loot_available[*i])
                .map(|(_, pos)| Vec3::from(*pos).distance(Vec3::from(local.pos)))
                .fold(f32::INFINITY, f32::min);
            let exit = Vec2::new(
                local.pos[0] - EXTRACTION_CENTER[0],
                local.pos[2] - EXTRACTION_CENTER[2],
            )
            .length();
            text.push_str(&format!(
                "IN RAID   {:02}:{:02}\nHead {:>2.0}/35  Thorax {:>2.0}/85  Arms {:>2.0}/60  Legs {:>2.0}/70\nVest {:.0} | Helmet {:.0} | Last hit: {:?}\n{} / {:?}   {} + {} reserve {}\nLoot {} | Secured {} | Exit {:.1}m\nPosition x {:.1} / z {:.1} | Exit x 0 / z 15\n",
                session.raid.seconds_left as u32 / 60, session.raid.seconds_left as u32 % 60,
                body.parts[0], body.parts[1], body.parts[2], body.parts[3],
                local.armor.vest, local.armor.helmet, body.last_hit,
                local.weapon.spec().name, p.loadout.ammo, local.ammo_in_mag, local.reserve_ammo,
                if local.reload_remaining > 0.0 { "[RELOADING]" } else { "" },
                p.carried, p.secured, exit, local.pos[0], local.pos[2],
            ));
            if nearest.is_finite() {
                text.push_str(&format!("Nearest orange crate: {:.1}m [F]\n", nearest));
            }
            if exit <= EXTRACTION_RADIUS {
                text.push_str(if p.carried == 0 {
                    "EXIT: collect loot before extracting"
                } else {
                    "EXIT: HOLD F"
                });
                text.push_str(&format!("  {:.1} / 5.0s\n", p.extraction_seconds));
            }
            text.push_str("WASD move | Shift run | Ctrl crouch | Space jump\nLMB fire | R reload | 1 primary / 2 pistol | F loot / extract\nWood can be penetrated. Concrete stops rounds. Esc: cursor.");
        }
        _ => {
            text.push_str(&format!(
                "{:?}\n\nRESULT: {:?}\nSecured this session: {}\nCarried: {}\n",
                session.raid.phase, p.status, p.secured, p.carried
            ));
            text.push_str(if session.raid.phase == RaidPhase::Debrief {
                "\n[Enter] Return everyone to lobby\nChoose equipment and ready for another raid."
            } else if p.status == RaidStatus::Waiting {
                "\nRaid in progress. Waiting for next lobby."
            } else {
                "\nWaiting for the other participants to finish.\nNo respawn in this raid."
            });
            text.push_str("\nEsc: release / capture cursor");
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lobby_and_result_explain_the_actual_next_action() {
        let mut session = ClientSession::default();
        let mut local = ClientData { id: 7, ..default() };
        session.players.push(local.clone());
        let text = describe(&session, &Loadout::default(), 7, "");
        assert!(text.contains("LOBBY"));
        assert!(text.contains("Ready / deploy"));
        assert!(text.contains("[G] Primary"));
        local.participant.status = RaidStatus::Extracted;
        local.participant.secured = 2;
        session.players[0] = local;
        session.raid.phase = RaidPhase::Debrief;
        let text = describe(&session, &Loadout::default(), 7, "");
        assert!(text.contains("Extracted"));
        assert!(text.contains("Secured this session: 2"));
        assert!(text.contains("[Enter] Return everyone to lobby"));
    }

    #[test]
    fn connection_failure_does_not_claim_a_live_raid() {
        let text = describe(
            &ClientSession::default(),
            &Loadout::default(),
            7,
            "DISCONNECTED",
        );
        assert!(text.contains("DISCONNECTED"));
        assert!(!text.contains("IN RAID"));
    }
}
