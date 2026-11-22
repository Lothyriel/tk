use super::*;
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

#[derive(Component)]
pub(super) struct PreviewTitle;

#[derive(Component)]
pub(super) struct Marker(pub usize);

impl Action {
    pub(crate) fn order(self) -> u16 {
        match self {
            Self::Slot => 0,
            Self::Reset => 1,
            Self::Close => 2,
            Self::Part(row) => 3 + 2 * row as u16,
            Self::Position(row) => 4 + 2 * row as u16,
            Self::Reserve(row) => 30 + row as u16,
            Self::Apply => 33,
        }
    }
}

fn button(parent: &mut ChildSpawnerCommands, action: Action, label: &str, width: f32) -> Entity {
    parent
        .spawn((
            crate::menu::widget(action.order()),
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
                Pickable::IGNORE,
            ));
        })
        .id()
}

pub(super) fn setup(mut commands: Commands, image: Res<preview::PreviewImage>) {
    commands
        .spawn((
            Panel,
            GlobalZIndex(300),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                top: Val::Px(60.0),
                bottom: Val::Px(16.0),
                left: Val::Px(16.0),
                right: Val::Px(16.0),
                padding: UiRect::all(Val::Px(16.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.018, 0.032, 0.048)),
        ))
        .with_children(|parent| {
            let mut targets = Vec::with_capacity(ROWS.len());
            parent.spawn((
                Text::new("WEAPON KIT / free equipment each raid"),
                TextFont::from_font_size(20.0),
            ));
            parent
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|parent| {
                    button(parent, Action::Slot, "Primary / Sidearm", 35.0);
                    button(parent, Action::Reset, "Discard draft", 30.0);
                    button(parent, Action::Close, "Back to lobby", 30.0);
                });
            parent
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(16.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    ..default()
                })
                .with_children(|parent| {
                    parent
                        .spawn((
                            crate::menu::BenchScroll,
                            bevy::ui_widgets::ScrollArea,
                            Node {
                                width: Val::Percent(52.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(4.0),
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                        ))
                        .with_children(|parent| {
                            for (index, _) in ROWS.iter().enumerate() {
                                parent
                                    .spawn((
                                        PartRow(index),
                                        Node {
                                            column_gap: Val::Px(4.0),
                                            flex_shrink: 0.0,
                                            ..default()
                                        },
                                    ))
                                    .with_children(|parent| {
                                        targets.push(button(parent, Action::Part(index), "", 80.0));
                                        button(parent, Action::Position(index), "", 19.0);
                                    });
                            }
                            for index in 0..3 {
                                button(parent, Action::Reserve(index), "", 100.0);
                            }
                            button(
                                parent,
                                Action::Apply,
                                "Apply draft (server validates)",
                                100.0,
                            );
                        });
                    parent
                        .spawn(Node {
                            width: Val::Percent(46.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(10.0),
                            ..default()
                        })
                        .with_children(|parent| {
                            parent.spawn((
                                PreviewTitle,
                                Text::new("Waiting for confirmed equipment..."),
                                TextFont::from_font_size(14.0),
                            ));
                            parent
                                .spawn((
                                    ImageNode::new(image.0.clone()),
                                    Node {
                                        width: Val::Percent(100.0),
                                        aspect_ratio: Some(1.6),
                                        flex_shrink: 0.0,
                                        ..default()
                                    },
                                ))
                                .with_children(|parent| {
                                    for (index, target) in targets.into_iter().enumerate() {
                                        parent
                                            .spawn((
                                                bevy::ui_widgets::Button,
                                                Marker(index),
                                                crate::menu::BenchFocusTarget(target),
                                                Node {
                                                    display: Display::None,
                                                    position_type: PositionType::Absolute,
                                                    width: Val::Px(21.0),
                                                    height: Val::Px(21.0),
                                                    align_items: AlignItems::Center,
                                                    justify_content: JustifyContent::Center,
                                                    border: UiRect::all(Val::Px(1.0)),
                                                    margin: UiRect {
                                                        left: Val::Px(-10.5),
                                                        top: Val::Px(-10.5),
                                                        ..default()
                                                    },
                                                    ..default()
                                                },
                                                BorderColor::all(Color::WHITE),
                                                BackgroundColor(Color::srgb(0.09, 0.16, 0.22)),
                                            ))
                                            .with_child((
                                                Text::new(format!("{}", index + 1)),
                                                TextFont::from_font_size(11.0),
                                                Pickable::IGNORE,
                                            ));
                                    }
                                });
                            parent.spawn((
                                Status,
                                Text::default(),
                                TextFont::from_font_size(14.0),
                                TextColor(Color::srgb(0.85, 0.93, 1.0)),
                            ));
                        });
                });
        });
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn refresh(
    (editor, session, id): (Res<Editor>, Res<ClientSession>, Res<PlayerId>),
    (preview, menu): (Res<preview::Preview>, Option<Res<crate::menu::Menu>>),
    mut panel: Single<&mut Node, (With<Panel>, Without<PartRow>, Without<Marker>)>,
    mut rows: Query<(&PartRow, &mut Node), (Without<Panel>, Without<Marker>)>,
    mut labels: Query<(&Label, &mut Text), (Without<Status>, Without<PreviewTitle>)>,
    mut status: Single<&mut Text, (With<Status>, Without<Label>, Without<PreviewTitle>)>,
    mut title: Query<&mut Text, (With<PreviewTitle>, Without<Status>, Without<Label>)>,
    buttons: Query<&Action>,
    mut marker_nodes: Query<
        (&Marker, &mut Node, &mut BackgroundColor, &Children),
        (Without<Panel>, Without<PartRow>),
    >,
    mut marker_text: Query<&mut TextColor, Without<Marker>>,
) {
    panel.display = if editor.visible {
        Display::Flex
    } else {
        Display::None
    };
    if !editor.visible {
        return;
    }
    let enabled =
        editor.editable(&session, id.0) && !menu.as_ref().is_some_and(|menu| menu.awaiting_lobby());
    let focused = menu
        .and_then(|menu| menu.focused_widget())
        .and_then(|entity| buttons.get(entity).ok());
    let row_focus = match focused {
        Some(Action::Part(row) | Action::Position(row)) => Some(*row),
        _ => None,
    };
    let title_text = match preview.view.as_ref() {
        Some(preview::View::Confirmed) => "CONFIRMED ASSEMBLY",
        Some(preview::View::Draft) => "DRAFT PREVIEW · not equipped",
        Some(preview::View::Pending) => "SUBMITTED DRAFT · waiting for server publication",
        Some(preview::View::Invalid(_)) => "INCOMPATIBLE DRAFT · showing confirmed assembly",
        Some(preview::View::Locked) => "CONFIRMED ASSEMBLY · editing locked",
        None => "Waiting for confirmed equipment...",
    };
    if let Ok(mut title) = title.single_mut() {
        **title = title_text.into();
    }
    for (marker, mut node, mut background, children) in &mut marker_nodes {
        let position = preview.socket(marker.0);
        if let Some(position) = position {
            node.display = Display::Flex;
            node.left = Val::Percent(position.x * 100.0);
            node.top = Val::Percent(position.y * 100.0);
        } else {
            node.display = Display::None;
        }
        let focused = row_focus == Some(marker.0);
        background.0 = if focused {
            Color::srgb(0.95, 0.83, 0.40)
        } else {
            Color::srgb(0.09, 0.16, 0.22)
        };
        if let Ok(mut color) = marker_text.get_mut(children[0]) {
            color.0 = if focused { Color::BLACK } else { Color::WHITE };
        }
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
            Action::Slot => format!("Editing {:?} / switch weapon", editor.slot),
            Action::Part(index) => {
                let row = &ROWS[index];
                let part = kit.weapon.components.iter().find(|p| p.node == row.node);
                format!(
                    "{} · {}: {}  >",
                    index + 1,
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
            Action::Close => "Back to lobby".into(),
            Action::Reset => "Discard draft".into(),
            Action::Apply => if editor.pending.is_some() {
                "Awaiting confirmation"
            } else {
                "Apply draft (server validates)"
            }
            .into(),
        };
    }
    let confirmed = session.configurations.cache.latest(id.0, editor.slot);
    let stats = confirmed.map_or_else(String::new, |entry| {
        format!(
            "Confirmed kit: {}\nMass {:.3} kg | Capacity {}\nOwned magazines: {:?}\nInstance {} / revision {}\n\n",
            entry.resolved.name,
            entry.resolved.assembly.mass_kg,
            entry.resolved.magazine_size,
            entry
                .packet
                .magazines
                .iter()
                .map(|m| catalog().component(m.definition).unwrap().capacity)
                .collect::<Vec<_>>(),
            entry.packet.instance.0,
            entry.packet.revision,
        )
    });
    ***status = format!(
        "{stats}{}\n\n{}\n\n{}",
        if enabled {
            match preview.view.as_ref() {
                Some(preview::View::Invalid(error)) => error,
                Some(preview::View::Draft) => "Draft valid. Apply to confirm.",
                _ => "",
            }
        } else {
            "LOCKED: ready/countdown/raid or pending request."
        },
        editor.status,
        "Click a number to focus its socket; click its row or press Enter to cycle.\nTab / arrows move focus. Removing a parent removes its draft descendants."
    );
}
