use super::*;
use bevy::ui_widgets::Button;
use common::catalog::{WeaponId, catalog};
use editor::{ROWS, part_name};

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Close,
    Slot,
    Part(usize),
    Position(usize),
    Reserve(usize),
    Reset,
    Apply,
}
#[derive(Component)]
pub(super) struct Panel;
#[derive(Component)]
pub(super) struct Label(Action);
#[derive(Component)]
pub(super) struct PartRow(usize);
#[derive(Component)]
pub(super) struct Status;

fn button(parent: &mut ChildSpawnerCommands, action: Action, label: &str, width: f32) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: Val::Percent(width),
                min_height: Val::Px(27.0),
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.09, 0.16, 0.22)),
        ))
        .with_children(|parent| {
            parent.spawn((
                Label(action),
                Text::new(label),
                TextFont::from_font_size(13.0),
                TextColor(Color::srgb(0.9, 0.97, 1.0)),
            ));
        });
}

pub(super) fn setup(mut commands: Commands, image: Res<preview::PreviewImage>) {
    commands.spawn((
        Panel, GlobalZIndex(300),
        Node { display: Display::None, position_type: PositionType::Absolute,
            top: Val::Px(60.0), bottom: Val::Px(16.0), left: Val::Px(16.0), right: Val::Px(16.0),
            padding: UiRect::all(Val::Px(16.0)), flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0), ..default() },
        BackgroundColor(Color::srgb(0.018, 0.032, 0.048)),
    )).with_children(|parent| {
        parent.spawn((Text::new("WEAPON KIT / free equipment each raid"),
            TextFont::from_font_size(20.0)));
        parent.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(8.0), ..default() })
            .with_children(|parent| {
                button(parent, Action::Slot, "Primary / Sidearm", 35.0);
                button(parent, Action::Reset, "Discard draft", 30.0);
                button(parent, Action::Close, "[V] Close", 30.0);
            });
        parent.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(16.0), flex_grow: 1.0, min_height: Val::Px(0.0), ..default() })
            .with_children(|parent| {
                parent.spawn(Node { width: Val::Percent(52.0), flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0), overflow: Overflow::scroll_y(), ..default() })
                    .with_children(|parent| {
                        for (index, _) in ROWS.iter().enumerate() {
                            parent.spawn((PartRow(index), Node { column_gap: Val::Px(4.0), flex_shrink: 0.0, ..default() }))
                                .with_children(|parent| {
                                    button(parent, Action::Part(index), "", 80.0);
                                    button(parent, Action::Position(index), "", 19.0);
                                });
                        }
                        for index in 0..3 { button(parent, Action::Reserve(index), "", 100.0); }
                        button(parent, Action::Apply, "Apply draft (server validates)", 100.0);
                    });
                parent.spawn(Node { width: Val::Percent(46.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(10.0), ..default() })
                    .with_children(|parent| {
                        parent.spawn((Text::new("CONFIRMED PREVIEW\nDraft changes appear only after server acceptance."),
                            TextFont::from_font_size(14.0)));
                        parent.spawn((ImageNode::new(image.0.clone()), Node {
                            width: Val::Percent(100.0), aspect_ratio: Some(1.6), ..default()
                        }));
                        parent.spawn((Status, Text::default(), TextFont::from_font_size(14.0),
                            TextColor(Color::srgb(0.85, 0.93, 1.0))));
                    });
            });
    });
}

#[allow(clippy::type_complexity)]
pub(super) fn refresh(
    (editor, session, id): (Res<Editor>, Res<ClientSession>, Res<PlayerId>),
    mut panel: Single<&mut Node, (With<Panel>, Without<PartRow>)>,
    mut rows: Query<(&PartRow, &mut Node), Without<Panel>>,
    mut labels: Query<(&Label, &mut Text), Without<Status>>,
    mut status: Single<&mut Text, (With<Status>, Without<Label>)>,
    mut buttons: Query<(&Action, &mut BackgroundColor)>,
    mut validation: Local<Option<(common::catalog::WeaponKit, String)>>,
) {
    panel.display = if editor.visible {
        Display::Flex
    } else {
        Display::None
    };
    if !editor.visible {
        return;
    }
    let enabled = editor.editable(&session, id.0);
    for (action, mut color) in &mut buttons {
        color.0 = if enabled || *action == Action::Close {
            Color::srgb(0.09, 0.16, 0.22)
        } else {
            Color::srgb(0.06, 0.07, 0.08)
        };
    }
    let Some(kit) = &editor.draft else {
        ***status = "Waiting for confirmed equipment...".into();
        return;
    };
    for (row, mut node) in &mut rows {
        node.display = if kit.weapon.definition == WeaponId::M4A1 || row.0 == 0 {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (label, mut text) in &mut labels {
        **text = match label.0 {
            Action::Slot => format!("Editing {:?} / click to switch", editor.slot),
            Action::Part(index) => {
                let row = &ROWS[index];
                let part = kit.weapon.components.iter().find(|p| p.node == row.node);
                format!(
                    "{}: {}  >",
                    row.name,
                    part_name(part.map_or(0, |p| p.definition.0))
                )
            }
            Action::Position(index) => {
                let part = kit
                    .weapon
                    .components
                    .iter()
                    .find(|p| p.node == ROWS[index].node);
                format!("Pos {} >", part.map_or(0, |p| p.position))
            }
            Action::Reserve(index) => format!(
                "Reserve {}: {}  >",
                index + 1,
                part_name(kit.reserves.get(index).map_or(0, |p| p.0))
            ),
            Action::Close => "[V] Close".into(),
            Action::Reset => "Discard draft".into(),
            Action::Apply => if editor.pending.is_some() {
                "Awaiting confirmation"
            } else {
                "Apply draft (server validates)"
            }
            .into(),
        };
    }
    if validation.as_ref().is_none_or(|(last, _)| last != kit) {
        let result = kit.resolve(catalog()).map_or_else(
            |e| format!("INCOMPATIBLE DRAFT\n{e}"),
            |_| "Draft valid. Apply to confirm.".into(),
        );
        *validation = Some((kit.clone(), result));
    }
    let confirmed = session.configurations.cache.latest(id.0, editor.slot);
    let stats = confirmed.map_or_else(String::new, |entry| {
        format!(
            "{}\nMass {:.3} kg | Capacity {}\nOwned magazines: {:?}\nInstance {} / revision {}\n\n",
            entry.resolved.name,
            entry.resolved.assembly.mass_kg,
            entry.resolved.magazine_size,
            entry
                .packet
                .magazines
                .iter()
                .map(|id| catalog().component(*id).unwrap().capacity)
                .collect::<Vec<_>>(),
            entry.packet.instance.0,
            entry.packet.revision,
        )
    });
    ***status = format!(
        "{stats}{}\n\n{}\n\n{}",
        if enabled {
            &validation.as_ref().unwrap().1
        } else {
            "LOCKED: ready/countdown/raid or pending request."
        },
        editor.status,
        "Click a part/reserve to cycle. Position cycles rail slots.\nRemoving a parent removes its draft descendants.\nClose, then Enter to ready. F/Q/E remain raid controls."
    );
}
