use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;

use bevy::ecs::event::Events;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use bevy::prelude::Messages;
use serde::Deserialize;
use serde_json::*;

use crate::constants::Flags;
use crate::core::{GameState, Game_State};
use crate::quadtree::aabb_collision;

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

#[derive(Event, Message)]
pub struct DialogueBoxTriggerEvent {}

#[derive(Event, Message)]
pub struct DialogueTriggerEvent {}

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
                    padding: UiRect::all(Val::Px(0.0)),
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
                            padding: UiRect::all(Val::Px(12.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.15, 0.15, 0.15)),
                        DialogueBox,
                    ))
                    .with_children(|box_node| {
                        box_node.spawn((
                            TextFont {
                                font_size: 16.0,
                                ..Default::default()
                            },
                            TextColor(Color::WHITE),
                            Text::new(""),
                            DialogueText,
                        ));
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
    box_query: Query<(Entity, &Children), With<DialogueBox>>,
    text_query: Query<Entity, With<DialogueText>>,
    button_query: Query<Entity, With<ChoiceButton>>,
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
                box_query,
                text_query,
                button_query,
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
    _asset_server: Res<AssetServer>,
    mut box_query: Query<(Entity, &Children), With<DialogueBox>>,
    text_query: Query<Entity, With<DialogueText>>,
    button_query: Query<Entity, With<ChoiceButton>>,
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
                box_query,
                text_query,
                button_query,
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
    mut commands: Commands,
    mut box_query: Query<(Entity, &Children), With<DialogueBox>>,
    text_query: Query<Entity, With<DialogueText>>,
    button_query: Query<Entity, With<ChoiceButton>>,
    mut index: ResMut<Selected_Choice_Index>,
    mut next_id_map: ResMut<Next_Id>,
    mut conditionals: ResMut<Conditionals>,
    mut events_dialogue_box: ResMut<Events<DialogueBoxTriggerEvent>>,
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
                            handle_choice_event(selected_choice.0.event, next_id_map, conditionals);
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
                            &mut commands,
                            &mut dialogue_state,
                            &dialogue_data,
                            box_query,
                            text_query,
                            button_query,
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
                            handle_choice_event(selected_choice.0.event, next_id_map, conditionals);
                        } else {
                            dialogue_state.0.current_id =
                                handle_next_id(line.next.clone(), &next_id_map);
                            if dialogue_state.0.current_id.is_none() {
                                dialogue_state.0.active = false;
                                game_state.0 = Game_State::Exploring;
                            }
                        }
                        display_dialogue(
                            &mut commands,
                            &mut dialogue_state,
                            &dialogue_data,
                            box_query,
                            text_query,
                            button_query,
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
    match File::open("dialogues/example.json") {
        Ok(file) => {
            let reader = BufReader::new(file);
            match serde_json::from_reader::<_, Vec<DialogueLine>>(reader) {
                Ok(dialogue_lines) => dialogue_lines
                    .into_iter()
                    .map(|line| (line.id.clone(), line))
                    .collect(),
                Err(err) => {
                    warn!("Failed to parse dialogues/example.json: {err}");
                    HashMap::new()
                }
            }
        }
        Err(err) => {
            warn!("Failed to open dialogues/example.json: {err}");
            HashMap::new()
        }
    }
}

pub fn display_dialogue(
    mut commands: &mut Commands,
    mut state: &mut ResMut<Dialogue_State>,
    data: &Res<Dialogue_Data>,
    mut box_query: Query<(Entity, &Children), With<DialogueBox>>,
    text_query: Query<Entity, With<DialogueText>>,
    button_query: Query<Entity, With<ChoiceButton>>,
    mut index: &mut ResMut<Selected_Choice_Index>,
    mut selected: &mut ResMut<Selected_Choice>,
) {
    if let Some(current_id) = &state.0.current_id {
        if let Some(dialogue) = data.0.get(current_id) {
            if let Ok((box_entity, children)) = box_query.single_mut() {
                for child in children.iter() {
                    if button_query.get(child).is_ok() {
                        commands.entity(child).despawn();
                    }
                }

                for child in children.iter() {
                    if text_query.get(child).is_ok() {
                        commands.entity(child).insert((
                            Text::new(format!("{}: {}", dialogue.speaker, dialogue.text)),
                            TextFont {
                                font_size: 20.0,
                                ..Default::default()
                            },
                            TextColor(Color::srgb(0.9, 0.9, 0.9)),
                            Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
                            GlobalTransform::default(),
                        ));
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
                                                width: Val::Px(240.0),
                                                height: Val::Px(45.0),
                                                margin: UiRect::vertical(Val::Px(4.0)),
                                                justify_content: JustifyContent::Center,
                                                align_items: AlignItems::Center,
                                                ..default()
                                            },
                                            BackgroundColor(if is_selected {
                                                Color::srgb(0.25, 0.45, 0.25)
                                            } else {
                                                Color::srgb(0.15, 0.25, 0.15)
                                            }),
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
                                                    Color::WHITE
                                                } else {
                                                    Color::srgb(0.7, 0.7, 0.7)
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
) {
    match event {
        0 => println!("Choice 1 selected"),
        1 => println!("Choice 2 selected"),
        _ => println!("Invalid choice"),
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
