use std::collections::HashMap;
use std::fs;
use std::path::Path;

use bevy::ecs::event::Events;
use bevy::ecs::system::SystemParam;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use bevy::prelude::Messages;
use serde::Deserialize;

use crate::constants::Flags;
use crate::core::{GameState, Game_State};
use crate::city_data::CityCatalog;
use crate::economy::Merchants;
use crate::governance::{ReputationChangeEvent, ReputationTarget};
use crate::map::CurrentArea;
use crate::quadtree::aabb_collision;
use crate::ui_style::{palette, radius, spacing};

const DIALOGUE_REPUTATION_RULES_PATH: &str = "assets/data/dialogue_reputation.ron";

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub enum DialogueSet {
    Spawn,
    Interact,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct Choice {
    pub event: u32,
    pub text: String,
    pub next: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DialogueLine {
    pub id: String,
    pub speaker: String,
    pub text: String,
    pub next: Option<String>,
    pub choices: Option<Vec<Choice>>,
}

#[derive(Debug)]
pub struct DialogueData(pub HashMap<String, DialogueLine>);

impl Default for DialogueData {
    fn default() -> Self {
        DialogueData(HashMap::new())
    }
}

pub struct DialogueState {
    pub current_id: Option<String>,
    pub active: bool,
    pub just_spawned: bool,
}

impl Default for DialogueState {
    fn default() -> Self {
        DialogueState {
            current_id: None,
            active: false,
            just_spawned: false,
        }
    }
}

#[derive(Resource, Default)]
pub struct Dialogue_State(pub DialogueState);

#[derive(Resource, Default)]
pub struct Dialogue_Data(pub HashMap<String, DialogueLine>);

#[derive(Resource, Default)]
pub struct Selected_Choice(pub Choice);

#[derive(Resource, Default)]
pub struct Selected_Choice_Index(pub Option<usize>);

#[derive(Resource, Default)]
pub struct Next_Id(pub HashMap<String, String>);

#[derive(Debug, Deserialize, Clone)]
struct DialogueReputationRuleFile {
    #[serde(default)]
    rules: Vec<DialogueReputationRuleEntry>,
}

#[derive(Debug, Deserialize, Clone)]
struct DialogueReputationRuleEntry {
    event_id: u32,
    #[serde(default)]
    effects: Vec<DialogueReputationEffect>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
enum DialogueReputationTargetKind {
    LocalGovernor,
    LocalMerchant,
    LocalClan,
}

#[derive(Debug, Deserialize, Clone)]
struct DialogueReputationEffect {
    target: DialogueReputationTargetKind,
    delta: i16,
    reason: String,
}

#[derive(Resource, Debug, Clone)]
pub struct DialogueReputationRules(HashMap<u32, Vec<DialogueReputationEffect>>);

impl Default for DialogueReputationRules {
    fn default() -> Self {
        if let Some(file_data) = load_dialogue_reputation_rules_file() {
            let mut out = HashMap::new();
            for rule in file_data.rules {
                if !rule.effects.is_empty() {
                    out.insert(rule.event_id, rule.effects);
                }
            }
            if !out.is_empty() {
                info!(
                    "Loaded dialogue reputation rules from {}",
                    DIALOGUE_REPUTATION_RULES_PATH
                );
                return Self(out);
            }
        }

        let mut out: HashMap<u32, Vec<DialogueReputationEffect>> = HashMap::new();
        out.insert(
            10,
            vec![DialogueReputationEffect {
                target: DialogueReputationTargetKind::LocalGovernor,
                delta: 10,
                reason: "dialogue_decision".to_string(),
            }],
        );
        out.insert(
            11,
            vec![DialogueReputationEffect {
                target: DialogueReputationTargetKind::LocalGovernor,
                delta: -10,
                reason: "dialogue_decision".to_string(),
            }],
        );
        out.insert(
            20,
            vec![DialogueReputationEffect {
                target: DialogueReputationTargetKind::LocalMerchant,
                delta: 8,
                reason: "dialogue_decision".to_string(),
            }],
        );
        out.insert(
            21,
            vec![DialogueReputationEffect {
                target: DialogueReputationTargetKind::LocalMerchant,
                delta: -8,
                reason: "dialogue_decision".to_string(),
            }],
        );
        out.insert(
            30,
            vec![DialogueReputationEffect {
                target: DialogueReputationTargetKind::LocalClan,
                delta: 12,
                reason: "dialogue_decision".to_string(),
            }],
        );
        out.insert(
            31,
            vec![DialogueReputationEffect {
                target: DialogueReputationTargetKind::LocalClan,
                delta: -12,
                reason: "dialogue_decision".to_string(),
            }],
        );
        Self(out)
    }
}

#[derive(Resource)]
pub struct Conditionals(pub Flags);

#[derive(Resource, Default)]
pub struct CachedInteractables(pub Vec<(Transform, Interactable)>);

#[derive(Component, Clone)]
pub struct Interactable {
    pub name: String,
    pub dialogue_id: String,
}

impl Interactable {
    pub fn interact(
        &self,
        _transform: &Transform,
        _game_state: Game_State,
        mut state: ResMut<Dialogue_State>,
        mut events_dialogue_box: ResMut<Messages<DialogueBoxTriggerEvent>>,
    ) {
        state.0.current_id = Some(self.dialogue_id.clone());
        state.0.active = true;
        let event = DialogueBoxTriggerEvent {};
        events_dialogue_box.send(event);
    }
}

#[derive(Component)]
pub struct DialogueText;

#[derive(Component)]
pub struct ChoiceButton {
    pub next_id: String,
}

#[derive(Component)]
pub struct DialogueBox;

#[derive(Component)]
pub struct DialoguePortrait;

#[derive(SystemParam)]
pub struct DialogueUiParams<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub box_query: Query<'w, 's, (Entity, &'static Children), With<DialogueBox>>,
    pub text_query: Query<'w, 's, Entity, With<DialogueText>>,
    pub button_query: Query<'w, 's, Entity, With<ChoiceButton>>,
    pub portrait_query:
        Query<'w, 's, (&'static mut ImageNode, &'static mut Visibility), With<DialoguePortrait>>,
    pub asset_server: Res<'w, AssetServer>,
}

#[derive(SystemParam)]
pub struct DialogueReputationParams<'w> {
    pub current_area: Res<'w, CurrentArea>,
    pub cities: Res<'w, CityCatalog>,
    pub merchants: Res<'w, Merchants>,
    pub reputation_events: ResMut<'w, Messages<ReputationChangeEvent>>,
    pub reputation_rules: Res<'w, DialogueReputationRules>,
}

#[derive(Event, Message)]
pub struct DialogueBoxTriggerEvent {}

#[derive(Event, Message)]
pub struct DialogueTriggerEvent {}

fn load_dialogue_reputation_rules_file() -> Option<DialogueReputationRuleFile> {
    let contents = match fs::read_to_string(DIALOGUE_REPUTATION_RULES_PATH) {
        Ok(s) => s,
        Err(err) => {
            warn!(
                "Failed to open {}: {}",
                DIALOGUE_REPUTATION_RULES_PATH, err
            );
            return None;
        }
    };
    match ron::de::from_str::<DialogueReputationRuleFile>(&contents) {
        Ok(data) => Some(data),
        Err(err) => {
            warn!(
                "Failed to parse {}: {}",
                DIALOGUE_REPUTATION_RULES_PATH, err
            );
            None
        }
    }
}

pub fn spawn_dialogue_box(
    mut commands: Commands,
    mut events_dialogue_box: ResMut<Messages<DialogueBoxTriggerEvent>>,
    mut events_dialogue: ResMut<Messages<DialogueTriggerEvent>>,
) {
    for _event in events_dialogue_box.drain() {
        commands
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::End,
                    align_items: AlignItems::Center,
                    padding: UiRect::all(Val::Px(spacing::LG)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|parent| {
                parent
                    .spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(200.0),
                            display: Display::Flex,
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::Start,
                            align_items: AlignItems::Start,
                            padding: UiRect::all(Val::Px(spacing::LG)),
                            border: UiRect::all(Val::Px(1.5)),
                            row_gap: Val::Px(spacing::SM),
                            ..default()
                        },
                        BackgroundColor(palette::BG_PANEL),
                        BorderRadius::all(Val::Px(radius::LG)),
                        BorderColor::all(palette::BORDER_ACCENT),
                        DialogueBox,
                    ))
                    .with_children(|box_node| {
                        box_node
                            .spawn((
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(110.0),
                                    display: Display::Flex,
                                    flex_direction: FlexDirection::Row,
                                    align_items: AlignItems::Center,
                                    column_gap: Val::Px(spacing::MD),
                                    ..default()
                                },
                                BackgroundColor(Color::NONE),
                            ))
                            .with_children(|row| {
                                row.spawn((
                                    Node {
                                        width: Val::Px(96.0),
                                        height: Val::Px(96.0),
                                        border: UiRect::all(Val::Px(1.0)),
                                        ..default()
                                    },
                                    ImageNode {
                                        image_mode: NodeImageMode::Stretch,
                                        ..default()
                                    },
                                    BorderRadius::all(Val::Px(radius::SM)),
                                    BorderColor::all(palette::BORDER_SUBTLE),
                                    Visibility::Hidden,
                                    DialoguePortrait,
                                ));
                                row.spawn((
                                    TextFont {
                                        font_size: 18.0,
                                        ..Default::default()
                                    },
                                    TextColor(palette::TEXT_PRIMARY),
                                    Text::new(""),
                                    DialogueText,
                                ));
                            });
                    });
            });
        let event = DialogueTriggerEvent {};
        events_dialogue.send(event);
    }
}

pub fn create_first_dialogue(
    mut commands: Commands,
    mut state: ResMut<Dialogue_State>,
    data: Res<Dialogue_Data>,
    asset_server: Res<AssetServer>,
    mut box_query: Query<(Entity, &Children), With<DialogueBox>>,
    text_query: Query<Entity, With<DialogueText>>,
    button_query: Query<Entity, With<ChoiceButton>>,
    mut portrait_query: Query<(&mut ImageNode, &mut Visibility), With<DialoguePortrait>>,
    mut index: ResMut<Selected_Choice_Index>,
    mut selected: ResMut<Selected_Choice>,
    mut events_dialogue: ResMut<Messages<DialogueTriggerEvent>>,
) {
    for _event in events_dialogue.drain() {
        state.0.just_spawned = true;
        index.0 = None;
        selected.0 = Choice::default();
    }

    if state.0.just_spawned {
        if box_query.iter().next().is_some() {
            display_dialogue(
                &mut commands,
                &mut state,
                &data,
                &asset_server,
                &mut box_query,
                &text_query,
                &button_query,
                &mut portrait_query,
                &mut index,
                &mut selected,
            );
            state.0.just_spawned = false;
        }
    }
}

pub fn gui_selection(
    input: Res<ButtonInput<KeyCode>>,
    game_state: Res<GameState>,
    mut index: ResMut<Selected_Choice_Index>,
    mut commands: Commands,
    mut state: ResMut<Dialogue_State>,
    data: Res<Dialogue_Data>,
    asset_server: Res<AssetServer>,
    mut box_query: Query<(Entity, &Children), With<DialogueBox>>,
    text_query: Query<Entity, With<DialogueText>>,
    button_query: Query<Entity, With<ChoiceButton>>,
    mut portrait_query: Query<(&mut ImageNode, &mut Visibility), With<DialoguePortrait>>,
    mut selected: ResMut<Selected_Choice>,
) {
    let moved = input.just_pressed(KeyCode::KeyW)
        || input.just_pressed(KeyCode::KeyS)
        || input.just_pressed(KeyCode::KeyA)
        || input.just_pressed(KeyCode::KeyD)
        || input.just_pressed(KeyCode::ArrowUp)
        || input.just_pressed(KeyCode::ArrowDown)
        || input.just_pressed(KeyCode::ArrowLeft)
        || input.just_pressed(KeyCode::ArrowRight);

    if !moved {
        return;
    }

    let vertical = (input.just_pressed(KeyCode::KeyS) || input.just_pressed(KeyCode::ArrowDown))
        as i32
        - (input.just_pressed(KeyCode::KeyW) || input.just_pressed(KeyCode::ArrowUp)) as i32;

    if vertical == 0 {
        return;
    }

    match game_state.0 {
        Game_State::Exploring => {}
        Game_State::Battle => {}
        Game_State::Interacting => {
            if let Some(current_id) = &state.0.current_id {
                if let Some(line) = data.0.get(current_id) {
                    if let Some(choices) = &line.choices {
                        if !choices.is_empty() {
                            let len = choices.len() as isize;
                            let next_index = match index.0 {
                                Some(i) => ((i as isize + vertical.signum() as isize)
                                    .rem_euclid(len)) as usize,
                                None => {
                                    if vertical > 0 {
                                        0
                                    } else {
                                        len.saturating_sub(1) as usize
                                    }
                                }
                            };
                            index.0 = Some(next_index);
                        }
                    }
                }
            }
            display_dialogue(
                &mut commands,
                &mut state,
                &data,
                &asset_server,
                &mut box_query,
                &text_query,
                &button_query,
                &mut portrait_query,
                &mut index,
                &mut selected,
            );
        }
        _ => {}
    }
}

pub fn interact(
    mut param_set: ParamSet<(
        Query<&Transform, With<crate::core::Player>>,
        Res<ButtonInput<KeyCode>>,
    )>,
    input: Res<ButtonInput<MouseButton>>,
    mut game_state: ResMut<GameState>,
    cache: Res<CachedInteractables>,
    mut dialogue_state: ResMut<Dialogue_State>,
    dialogue_data: Res<Dialogue_Data>,
    mut selected_choice: ResMut<Selected_Choice>,
    mut ui: DialogueUiParams,
    mut index: ResMut<Selected_Choice_Index>,
    mut next_id_map: ResMut<Next_Id>,
    mut conditionals: ResMut<Conditionals>,
    mut events_dialogue_box: ResMut<Events<DialogueBoxTriggerEvent>>,
    mut reputation: DialogueReputationParams,
) {
    if (param_set.p1().just_pressed(KeyCode::KeyX)) {
        match game_state.0 {
            Game_State::Exploring => {
                let p0 = param_set.p0();

                for transform in p0.iter() {
                    let player_rect = Rect::from_center_size(
                        Vec2::new(transform.translation.x, transform.translation.y),
                        Vec2::new(32.0, 32.0),
                    );

                    let interactable: Option<&(Transform, Interactable)> =
                        cache.0.iter().find(|(interactable_transform, interactable)| {
                            let wall_rect = Rect::from_center_size(
                                Vec2::new(
                                    interactable_transform.translation.x,
                                    interactable_transform.translation.y,
                                ),
                                Vec2::new(32.0, 32.0),
                            );
                            aabb_collision(player_rect, wall_rect)
                        });

                    if let Some((interactable_transform, interactable)) = interactable {
                        game_state.0 = Game_State::Interacting;
                        dialogue_state.0.active = true;
                        index.0 = None;
                        selected_choice.0 = Choice::default();
                        interactable.interact(
                            interactable_transform,
                            game_state.0,
                            dialogue_state,
                            events_dialogue_box,
                        );
                        break;
                    }
                }
            }
            Game_State::Battle => {}
            Game_State::Interacting => {
                if let Some(current_id) = &dialogue_state.0.current_id {
                    if let Some(line) = dialogue_data.0.get(current_id) {
                        let current_choices = &line.choices;
                        if current_choices.is_some() {
                            if selected_choice.0.next.is_none() {
                                return;
                            }
                            dialogue_state.0.current_id =
                                handle_next_id(selected_choice.0.next.clone(), &next_id_map);
                            handle_choice_event(
                                selected_choice.0.event,
                                next_id_map,
                                conditionals,
                                reputation.current_area.0,
                                &reputation.cities,
                                &reputation.merchants,
                                &mut reputation.reputation_events,
                                &reputation.reputation_rules,
                            );
                        } else {
                            dialogue_state.0.current_id =
                                handle_next_id(line.next.clone(), &next_id_map);
                            if dialogue_state.0.current_id.is_none() {
                                dialogue_state.0.active = false;
                                game_state.0 = Game_State::Exploring;
                            }
                        }
                        index.0 = None;
                        selected_choice.0 = Choice::default();
                        display_dialogue(
                            &mut ui.commands,
                            &mut dialogue_state,
                            &dialogue_data,
                            &ui.asset_server,
                            &mut ui.box_query,
                            &ui.text_query,
                            &ui.button_query,
                            &mut ui.portrait_query,
                            &mut index,
                            &mut selected_choice,
                        );
                    }
                }
            }
            _ => {}
        }
    } else if param_set.p1().just_pressed(KeyCode::Space)
        || param_set.p1().just_pressed(KeyCode::Enter)
        || input.just_pressed(MouseButton::Left)
    {
        match game_state.0 {
            Game_State::Exploring => {}
            Game_State::Battle => {}
            Game_State::Interacting => {
                if let Some(current_id) = &dialogue_state.0.current_id {
                    if let Some(line) = dialogue_data.0.get(current_id) {
                        let current_choices = &line.choices;
                        if current_choices.is_some() {
                            if selected_choice.0.next.is_none() {
                                return;
                            }
                            dialogue_state.0.current_id =
                                handle_next_id(selected_choice.0.next.clone(), &next_id_map);
                            handle_choice_event(
                                selected_choice.0.event,
                                next_id_map,
                                conditionals,
                                reputation.current_area.0,
                                &reputation.cities,
                                &reputation.merchants,
                                &mut reputation.reputation_events,
                                &reputation.reputation_rules,
                            );
                        } else {
                            dialogue_state.0.current_id =
                                handle_next_id(line.next.clone(), &next_id_map);
                            if dialogue_state.0.current_id.is_none() {
                                dialogue_state.0.active = false;
                                game_state.0 = Game_State::Exploring;
                            }
                        }
                        display_dialogue(
                            &mut ui.commands,
                            &mut dialogue_state,
                            &dialogue_data,
                            &ui.asset_server,
                            &mut ui.box_query,
                            &ui.text_query,
                            &ui.button_query,
                            &mut ui.portrait_query,
                            &mut index,
                            &mut selected_choice,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn load_dialogue() -> HashMap<String, DialogueLine> {
    match fs::read_to_string("dialogues/example.ron") {
        Ok(contents) => match ron::de::from_str::<Vec<DialogueLine>>(&contents) {
            Ok(dialogue_lines) => dialogue_lines
                .into_iter()
                .map(|line| (line.id.clone(), line))
                .collect(),
            Err(err) => {
                warn!("Failed to parse dialogues/example.ron: {err}");
                HashMap::new()
            }
        },
        Err(err) => {
            warn!("Failed to open dialogues/example.ron: {err}");
            HashMap::new()
        }
    }
}

pub fn display_dialogue(
    commands: &mut Commands,
    state: &mut ResMut<Dialogue_State>,
    data: &Res<Dialogue_Data>,
    asset_server: &Res<AssetServer>,
    box_query: &mut Query<(Entity, &Children), With<DialogueBox>>,
    text_query: &Query<Entity, With<DialogueText>>,
    button_query: &Query<Entity, With<ChoiceButton>>,
    portrait_query: &mut Query<(&mut ImageNode, &mut Visibility), With<DialoguePortrait>>,
    index: &mut ResMut<Selected_Choice_Index>,
    selected: &mut ResMut<Selected_Choice>,
) {
    if let Some(current_id) = &state.0.current_id {
        if let Some(dialogue) = data.0.get(current_id) {
            if let Ok((box_entity, children)) = box_query.single_mut() {
                for child in children.iter() {
                    if button_query.get(child).is_ok() {
                        commands.entity(child).despawn();
                    }
                }

                if let Ok(text_entity) = text_query.single() {
                    commands.entity(text_entity).insert((
                        Text::new(format!("{}: {}", dialogue.speaker, dialogue.text)),
                        TextFont {
                            font_size: 20.0,
                            ..Default::default()
                        },
                        TextColor(palette::TEXT_PRIMARY),
                        Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
                        GlobalTransform::default(),
                    ));
                }

                if let Ok((mut image_node, mut visibility)) = portrait_query.single_mut() {
                    if let Some(path) = portrait_asset_path(&dialogue.speaker) {
                        image_node.image = asset_server.load(path);
                        image_node.image_mode = NodeImageMode::Stretch;
                        *visibility = Visibility::Visible;
                    } else {
                        *visibility = Visibility::Hidden;
                    }
                }

                let choices = &dialogue.choices;

                match choices {
                    Some(choices_v) => {
                        if let Some(current) = index.0 {
                            if current >= choices_v.len() {
                                index.0 = None;
                            }
                        }

                        if index.0.is_none() {
                            selected.0 = Choice::default();
                        }

                        for (i, choice) in choices_v.iter().enumerate() {
                            let is_selected = index.0 == Some(i);
                            if is_selected {
                                selected.0 = choice.clone();
                            }
                            commands.entity(box_entity).with_children(|parent| {
                                if let Some(next_id) = choice.next.as_ref() {
                                    parent
                                        .spawn((
                                            Button { ..default() },
                                            Node {
                                                width: Val::Px(280.0),
                                                height: Val::Px(40.0),
                                                margin: UiRect::vertical(Val::Px(spacing::XS)),
                                                padding: UiRect::horizontal(Val::Px(spacing::MD)),
                                                justify_content: JustifyContent::Center,
                                                align_items: AlignItems::Center,
                                                ..default()
                                            },
                                            BackgroundColor(if is_selected {
                                                palette::BG_BUTTON_PRESSED
                                            } else {
                                                palette::BG_BUTTON
                                            }),
                                            BorderRadius::all(Val::Px(radius::MD)),
                                            ChoiceButton {
                                                next_id: next_id.clone(),
                                            },
                                        ))
                                        .with_children(|btn| {
                                            btn.spawn((
                                                Text::new(&choice.text),
                                                TextFont {
                                                    font_size: 17.0,
                                                    ..Default::default()
                                                },
                                                TextColor(if is_selected {
                                                    palette::TEXT_HEADING
                                                } else {
                                                    palette::TEXT_SECONDARY
                                                }),
                                                Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
                                                GlobalTransform::default(),
                                            ));
                                        });
                                } else {
                                    warn!(
                                        "Choice '{}' missing next id; skipping button spawn",
                                        choice.text
                                    );
                                }
                            });
                        }
                    }

                    None => {}
                }
            }
        }
    } else {
        for (box_entity, children) in box_query.iter_mut() {
            for child in children.iter() {
                if button_query.get(child).is_ok() {
                    commands.entity(child).despawn();
                }
            }
            commands.entity(box_entity).despawn();
        }
    }
}

fn handle_choice_event(
    event: u32,
    mut _next_id_map: ResMut<Next_Id>,
    mut _conditionals: ResMut<Conditionals>,
    current_region: u16,
    cities: &Res<CityCatalog>,
    merchants: &Res<Merchants>,
    reputation_events: &mut ResMut<Messages<ReputationChangeEvent>>,
    reputation_rules: &Res<DialogueReputationRules>,
) {
    let Some(effects) = reputation_rules.0.get(&event) else {
        return;
    };
    let local_city = cities
        .0
        .values()
        .find(|city| city.region_ids.contains(&current_region));
    let local_merchant_id = merchants
        .0
        .iter()
        .find(|(_, merchant)| merchant.region_id == current_region)
        .map(|(merchant_id, _)| *merchant_id);

    for effect in effects {
        match effect.target {
            DialogueReputationTargetKind::LocalGovernor => {
                let Some(city) = local_city else {
                    continue;
                };
                reputation_events.write(ReputationChangeEvent {
                    target: ReputationTarget::Governor { city_id: city.id },
                    delta: effect.delta,
                    reason: effect.reason.clone(),
                });
            }
            DialogueReputationTargetKind::LocalMerchant => {
                let Some(merchant_id) = local_merchant_id else {
                    continue;
                };
                reputation_events.write(ReputationChangeEvent {
                    target: ReputationTarget::Merchant { merchant_id },
                    delta: effect.delta,
                    reason: effect.reason.clone(),
                });
            }
            DialogueReputationTargetKind::LocalClan => {
                let Some(city) = local_city else {
                    continue;
                };
                reputation_events.write(ReputationChangeEvent {
                    target: ReputationTarget::Clan {
                        clan_name: city.clan_name.clone(),
                    },
                    delta: effect.delta,
                    reason: effect.reason.clone(),
                });
            }
        }
    }
}

fn handle_next_id(id: Option<String>, next_id_map: &ResMut<Next_Id>) -> Option<String> {
    let return_id = match id {
        None => None,
        Some(id) => {
            let next_id = match next_id_map.0.get(&id) {
                None => Some(id.clone()),
                Some(next_id) => Some(next_id.clone()),
            };
            next_id
        }
    };
    return_id
}

fn portrait_asset_path(speaker: &str) -> Option<String> {
    let speaker = speaker.trim();
    if speaker.is_empty() {
        return None;
    }

    let base_dir = Path::new("assets").join("portraits");
    let exts = ["png", "jpg", "jpeg", "webp"];

    for ext in exts {
        let file_name = format!("{}.{}", speaker, ext);
        let fs_path = base_dir.join(&file_name);
        if fs_path.is_file() {
            return Some(format!("portraits/{}", file_name));
        }
    }

    None
}
