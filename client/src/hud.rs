use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
};
use bevy_renet2::prelude::RenetClient;
use common::{combat::*, map::*, raid::*, *};

use crate::{ClientSystems, sync::ClientSession};

mod combat;

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    update.in_set(ClientSystems::Present),
                    update_metrics
                        .after(FrameTimeDiagnosticsPlugin::diagnostic_system)
                        .in_set(ClientSystems::Present),
                ),
            );
    }
}

#[derive(Component)]
pub(crate) struct RaidHud;

#[derive(Component)]
struct MetricsHud;

fn setup(mut commands: Commands) {
    commands.spawn((
        RaidHud,
        Text::new("TK / EXTRACTION ALPHA\nConnecting to server..."),
        TextFont::from_font_size(16.0),
        TextColor(Color::srgb(0.9, 0.97, 1.0)),
        BackgroundColor(Color::srgba(0.015, 0.025, 0.04, 0.88)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(18.0),
            left: Val::Px(18.0),
            max_width: Val::Px(320.0),
            padding: UiRect::all(Val::Px(12.0)),
            ..default()
        },
        GlobalZIndex(200),
    ));
    commands.spawn((
        MetricsHud,
        Text::new("FPS --\nFRAME -- ms"),
        TextFont::from_font_size(14.0),
        TextColor(Color::srgb(0.9, 0.97, 1.0)),
        BackgroundColor(Color::srgba(0.015, 0.025, 0.04, 0.72)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(18.0),
            right: Val::Px(18.0),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(7.0)),
            ..default()
        },
        GlobalZIndex(200),
    ));
}

fn update_metrics(
    diagnostics: Res<DiagnosticsStore>,
    mut hud: Single<&mut Text, With<MetricsHud>>,
) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|diagnostic| diagnostic.smoothed());
    let frame_time = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|diagnostic| diagnostic.smoothed());
    ***hud = metrics_text(fps, frame_time);
}

fn metrics_text(fps: Option<f64>, frame_time_ms: Option<f64>) -> String {
    match (fps, frame_time_ms) {
        (Some(fps), Some(frame_time_ms)) => {
            format!("FPS {fps:>5.0}\nFRAME {frame_time_ms:>5.1} ms")
        }
        _ => "FPS --\nFRAME -- ms".to_string(),
    }
}

fn update(
    time: Res<Time<Real>>,
    session: Res<ClientSession>,
    input: Res<LocalInput>,
    id: Res<PlayerId>,
    client: Res<RenetClient>,
    (inventory, panel, menu): (
        Res<crate::sync::inventory::State>,
        Res<crate::input::inventory_panel::Panel>,
        Res<crate::menu::Menu>,
    ),
    mut hud: Single<&mut Text, With<RaidHud>>,
) {
    if menu.replaces_hud() {
        ***hud = String::new();
        return;
    }
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
    let mut text = describe(&session, &input.loadout, id.0, &connection, input.aim);
    if connection.is_empty() && panel.open {
        use common::catalog::item_manifest::planned_items;
        text = if panel.inspection.is_some() {
            String::from("ITEM INSPECTION\nArrows: rotate   +/-: zoom   R: reset   I/Esc: back\n")
        } else {
            String::from(
                "RAID INVENTORY\nUp/Down: select   I: inspect   Delete: drop   Tab/Esc: close\n",
            )
        };
        match inventory.owned_grid(session.raid.raid_id, id.0) {
            Some(grid) if grid.items.is_empty() => text.push_str("\nEmpty"),
            Some(grid) => {
                for (instance, item) in &grid.items {
                    if panel
                        .inspection
                        .is_some_and(|inspect| inspect.item != *instance)
                    {
                        continue;
                    }
                    let marker = if panel.selected == Some(*instance) {
                        ">"
                    } else {
                        " "
                    };
                    let name = planned_items()
                        .get(item.definition.as_str())
                        .map_or(item.definition.as_str(), |entry| entry.name);
                    text.push_str(&format!("\n{marker} {name} ×{}", item.quantity));
                }
            }
            None => text.push_str("\nWaiting for inventory"),
        }
    } else if connection.is_empty()
        && session.raid.phase == RaidPhase::Active
        && session
            .local(id.0)
            .is_some_and(|actor| actor.participant.status == RaidStatus::Active)
        && let Some(item) = inventory.focused_ground(session.raid.raid_id)
    {
        use common::catalog::item_manifest::{S1_SEEDS, planned_items};
        let name = planned_items()[S1_SEEDS[usize::from(item.seed)]].name;
        text.push_str(&format!("\nGround item: {name} [F]"));
    }
    ***hud = text;
}

fn describe(
    session: &ClientSession,
    pending: &Loadout,
    id: u64,
    connection: &str,
    aiming: bool,
) -> String {
    let arena = session.raid.mode != MatchMode::Raid;
    let mut text = format!(
        "TK / EXTRACTION ALPHA   |   {} {}\n",
        if arena { "ARENA" } else { "RAID" },
        session.raid.raid_id
    );
    if !connection.is_empty() {
        text.push_str(connection);
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
                "\n\n[G] Primary: {}  +  Pistol\n[B] Ammo: {:?} (AP trades damage for penetration)\n[H] Vest: {:?}    [J] Helmet: {}\n[V] Configure weapon parts / magazines\n\n[Enter] {}\n",
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
            if local.mechanics.action_unavailable() {
                if local.mechanics.recovery_ms() == 0 {
                    text.push_str("ACTION UNAVAILABLE | Recover using reload\n");
                } else {
                    text.push_str(&format!(
                        "RECOVERING | {:.1}s\n",
                        f32::from(local.mechanics.recovery_ms()) / 1000.0,
                    ));
                }
            }
            combat::append_readout(&mut text, local);
            if arena {
                if !aiming && local.pose.ads <= 0.02 {
                    text.push_str("\nEsc: pause or leave Arena");
                }
                return text;
            }
            let exit = Vec2::new(
                local.pos[0] - EXTRACTION_CENTER[0],
                local.pos[2] - EXTRACTION_CENTER[2],
            )
            .length();
            text.push_str(&format!(
                "\n{:02}:{:02}  |  Loot {}  |  Exit {:.1}m",
                session.raid.seconds_left as u32 / 60,
                session.raid.seconds_left as u32 % 60,
                p.carried,
                exit,
            ));
            if !aiming && local.pose.ads <= 0.02 {
                let nearest = LOOT_POSITIONS
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| session.raid.loot_available[*i])
                    .map(|(_, pos)| Vec3::from(*pos).distance(Vec3::from(local.pos)))
                    .fold(f32::INFINITY, f32::min);
                if nearest.is_finite() {
                    text.push_str(&format!("\nNearest orange crate: {nearest:.1}m [F]"));
                }
            }
            append_exit(&mut text, exit, p);
        }
        _ => {
            text.push_str(&format!(
                "{:?}\n\nRESULT: {:?}\nSecured this session: {}\nCarried: {}\n",
                session.raid.phase, p.status, p.secured, p.carried
            ));
            text.push_str(if session.raid.phase == RaidPhase::Debrief {
                "\nReturn to lobby to prepare another raid."
            } else if p.status == RaidStatus::Waiting {
                "\nRaid in progress. Waiting for next lobby."
            } else {
                "\nWaiting for the other participants to finish.\nNo respawn in this raid."
            });
        }
    }
    text
}

fn append_exit(text: &mut String, distance: f32, participant: &Participant) {
    if distance <= EXTRACTION_RADIUS {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(if participant.carried == 0 {
            "EXIT: collect loot before extracting"
        } else {
            "EXIT: HOLD F"
        });
        text.push_str(&format!("  {:.1} / 5.0s\n", participant.extraction_seconds));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raid_hud_distinguishes_chamber_inserted_rounds_and_detached_magazine() {
        use common::catalog::{MagazineSnapshot, ManipulationKind, ManipulationPhase};
        let mut session = ClientSession::default();
        session.raid.phase = RaidPhase::Active;
        let mut local = ClientData { id: 7, ..default() };
        local.participant.begin();
        local.mechanics.chambered = true;
        local.mechanics.magazine_inserted = true;
        local.mechanics.magazine = MagazineSnapshot::new(0, 30);
        session.players.push(local);
        assert!(
            describe(&session, &Loadout::default(), 7, "", false)
                .contains("Chamber loaded | Magazine 30")
        );
        session.players[0].mechanics.magazine = MagazineSnapshot::new(0, 29);
        assert!(describe(&session, &Loadout::default(), 7, "", false).contains("Magazine 29"));
        let mechanics = &mut session.players[0].mechanics;
        mechanics.kind = Some(ManipulationKind::TacticalReload);
        mechanics.magazine_inserted = false;
        mechanics.reload_target = MagazineSnapshot::new(1, 20);
        assert!(
            describe(&session, &Loadout::default(), 7, "", false)
                .contains("Chamber loaded | Magazine absent")
        );
        let mechanics = &mut session.players[0].mechanics;
        mechanics.phase = Some(ManipulationPhase::Feed);
        mechanics.magazine_inserted = true;
        let text = describe(&session, &Loadout::default(), 7, "", false);
        assert!(text.contains("Reloading") && text.contains("Chamber loaded | Magazine 20"));
        let player = &mut session.players[0];
        player.weapon_id = WeaponId::M1911;
        player.weapon_slot = EquipmentSlot::Sidearm;
        player.mechanics.kind = None;
        player.mechanics.phase = None;
        player.mechanics.magazine = MagazineSnapshot::new(0, 0);
        player.mechanics.chambered = false;
        player.reserve_ammo = 77; // Not a usable count for the magazine.
        let text = describe(&session, &Loadout::default(), 7, "", false);
        assert!(text.contains("Pistol") && !text.contains("M4A1"));
        assert!(text.contains("Chamber empty | Magazine 0"));
        assert!(!text.contains("Reloading") && !text.contains("77"));
        session.players[0].mechanics.magazine = MagazineSnapshot::default();
        assert!(describe(&session, &Loadout::default(), 7, "", false).contains("Magazine unknown"));
    }

    #[test]
    fn raid_hud_compacts_during_ads_entry_and_release_without_losing_exit() {
        let mut session = ClientSession::default();
        session.raid.phase = RaidPhase::Active;
        let mut local = ClientData {
            id: 7,
            pos: EXTRACTION_CENTER,
            ammo_in_mag: 23,
            reserve_ammo: 47,
            ..default()
        };
        local.participant.begin();
        local.participant.carried = 1;
        session.players.push(local);
        let text = describe(&session, &Loadout::default(), 7, "", true);
        assert!(text.contains("HP 445/445") && text.contains("EXIT: HOLD F"));
        assert!(!text.contains("Nearest orange crate"));
        session.players[0].pose.ads = 0.1;
        assert_eq!(describe(&session, &Loadout::default(), 7, "", false), text);
        session.players[0].pose.ads = 0.0;
        assert!(
            describe(&session, &Loadout::default(), 7, "", false).contains("Nearest orange crate")
        );
    }

    #[test]
    fn combat_hud_clears_previous_match_values_on_death_lobby_or_stale_connection() {
        let mut session = ClientSession::default();
        session.raid.phase = RaidPhase::Active;
        let mut local = ClientData { id: 7, ..default() };
        local.participant.begin();
        local.body.anatomy.regions[0] = 5.0;
        session.players.push(local);
        let view = |session: &ClientSession, connection| {
            describe(session, &Loadout::default(), 7, connection, false)
        };
        assert!(view(&session, "").contains("Head: Critical"));
        let assert_no_combat = |text: String| {
            for field in ["HP ", "Blood ", "Head: ", "Chamber ", "Magazine "] {
                assert!(!text.contains(field), "stale field {field}: {text}");
            }
        };
        assert_no_combat(view(&session, "CONNECTION STALE"));
        session.players[0].participant.status = RaidStatus::Dead;
        assert_no_combat(view(&session, ""));
        session.raid.phase = RaidPhase::Lobby;
        session.players[0].participant.return_to_lobby();
        assert_no_combat(view(&session, ""));
    }

    #[test]
    fn performance_overlay_formats_smoothed_fps_and_frame_time() {
        assert_eq!(metrics_text(None, None), "FPS --\nFRAME -- ms");
        let text = metrics_text(Some(60.0), Some(16.7));
        assert!(text.contains("FPS    60"));
        assert!(text.contains("FRAME  16.7 ms"));
    }

    #[test]
    fn lobby_and_result_explain_the_actual_next_action() {
        let mut session = ClientSession::default();
        let mut local = ClientData { id: 7, ..default() };
        session.players.push(local.clone());
        let text = describe(&session, &Loadout::default(), 7, "", false);
        assert!(text.contains("LOBBY"));
        assert!(text.contains("Ready / deploy"));
        assert!(text.contains("[G] Primary"));
        local.participant.status = RaidStatus::Extracted;
        local.participant.secured = 2;
        session.players[0] = local;
        session.raid.phase = RaidPhase::Debrief;
        let text = describe(&session, &Loadout::default(), 7, "", false);
        assert!(text.contains("Extracted"));
        assert!(text.contains("Secured this session: 2"));
        assert!(text.contains("Return to lobby to prepare another raid."));
    }

    #[test]
    fn connection_failure_does_not_claim_a_live_raid() {
        let text = describe(
            &ClientSession::default(),
            &Loadout::default(),
            7,
            "DISCONNECTED",
            true,
        );
        assert!(text.contains("DISCONNECTED"));
        assert!(!text.contains("IN RAID"));
    }
}
