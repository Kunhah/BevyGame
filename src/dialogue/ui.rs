use bevy::ecs::system::SystemParam;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use bevy::prelude::Messages;

use crate::core::{GameState, Game_State, Player};
use crate::quadtree::aabb_collision;
use crate::quests::DialogueChoicePickedEvent;
use crate::ui_style::{palette, radius, spacing};

use super::runtime::{
    ConditionContext, DialogueCatalog, DialogueRuntime, DialogueSelectedIndex, EffectDispatcher,
    evaluate_condition,
};
use super::scene_player::{start_scene_playback_if_needed, ScenePlayback};
use super::schema::{ChoiceNode, ChoiceOption, DialogueNode, LineNode, SceneNode, Speaker};
use super::stage::spawn_stage_overlays;

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub enum DialogueSet {
    Spawn,
    Interact,
}

#[derive(Event, Message)]
pub struct DialogueBoxTriggerEvent;

#[derive(Event, Message)]
pub struct DialogueTriggerEvent;

#[derive(Component, Clone)]
pub struct Interactable {
    /// Authoring-side label. Carried for editor and debug tools; unread by
    /// runtime today.
    #[allow(dead_code)]
    pub name: String,
    /// Scene id this interactable opens.
    pub dialogue_id: String,
}

#[derive(Resource, Default)]
pub struct CachedInteractables(pub Vec<(Transform, Interactable)>);

#[derive(Component)]
pub struct DialogueText;

#[derive(Component)]
pub struct ChoiceButton;

#[derive(Component)]
pub struct DialogueBox;

// ---------------------------------------------------------------------------
// UI param bundle (text-only now; portraits are owned by the stage)
// ---------------------------------------------------------------------------

#[derive(SystemParam)]
pub struct DialogueUiParams<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub box_query: Query<'w, 's, (Entity, &'static Children), With<DialogueBox>>,
    pub text_query: Query<'w, 's, Entity, With<DialogueText>>,
    pub button_query: Query<'w, 's, Entity, With<ChoiceButton>>,
}

// ---------------------------------------------------------------------------
// Spawn / first-render systems
// ---------------------------------------------------------------------------

pub fn spawn_dialogue_box(
    mut commands: Commands,
    mut events_dialogue_box: ResMut<Messages<DialogueBoxTriggerEvent>>,
    mut events_dialogue: ResMut<Messages<DialogueTriggerEvent>>,
) {
    if events_dialogue_box.drain().next().is_none() {
        return;
    }

    spawn_stage_overlays(&mut commands);

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
                position_type: PositionType::Absolute,
                ..default()
            },
            BackgroundColor(Color::NONE),
            ZIndex(10),
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
                    box_node.spawn((
                        TextFont {
                            font_size: 20.0,
                            ..Default::default()
                        },
                        TextColor(palette::TEXT_PRIMARY),
                        Text::new(""),
                        DialogueText,
                    ));
                });
        });
    events_dialogue.write(DialogueTriggerEvent);
}

pub fn create_first_dialogue(
    mut runtime: ResMut<DialogueRuntime>,
    catalog: Res<DialogueCatalog>,
    mut index: ResMut<DialogueSelectedIndex>,
    mut playback: ResMut<ScenePlayback>,
    cond_ctx: ConditionContext,
    mut ui: DialogueUiParams,
    mut events_dialogue: ResMut<Messages<DialogueTriggerEvent>>,
) {
    for _event in events_dialogue.drain() {
        runtime.just_spawned = true;
        index.0 = None;
    }

    if runtime.just_spawned && ui.box_query.iter().next().is_some() {
        // Hand off to the scene player if the entry node is a Scene; the
        // dialogue box stays empty in that case until the timeline drops us
        // onto a Line/Choice.
        start_scene_playback_if_needed(&runtime, &catalog, &mut playback);
        display_dialogue(&runtime, &catalog, &cond_ctx, index.0, &mut ui);
        runtime.just_spawned = false;
    }
}

// ---------------------------------------------------------------------------
// Input: choice navigation (W/S, arrows)
// ---------------------------------------------------------------------------

pub fn gui_selection(
    input: Res<ButtonInput<KeyCode>>,
    game_state: Res<GameState>,
    mut index: ResMut<DialogueSelectedIndex>,
    runtime: Res<DialogueRuntime>,
    catalog: Res<DialogueCatalog>,
    playback: Res<ScenePlayback>,
    cond_ctx: ConditionContext,
    mut ui: DialogueUiParams,
) {
    if !matches!(game_state.0, Game_State::Interacting) || playback.blocking() {
        return;
    }

    let vertical = (input.just_pressed(KeyCode::KeyS) || input.just_pressed(KeyCode::ArrowDown))
        as i32
        - (input.just_pressed(KeyCode::KeyW) || input.just_pressed(KeyCode::ArrowUp)) as i32;

    if vertical == 0 {
        return;
    }

    let Some(DialogueNode::Choice(choice)) = runtime.current_node(&catalog) else {
        return;
    };
    let visible = visible_options(&choice.options, &cond_ctx);
    if visible.is_empty() {
        return;
    }

    let len = visible.len() as isize;
    let next_visible_idx = match find_visible_index(&visible, index.0) {
        Some(i) => ((i as isize + vertical.signum() as isize).rem_euclid(len)) as usize,
        None => {
            if vertical > 0 {
                0
            } else {
                (len - 1) as usize
            }
        }
    };
    index.0 = Some(visible[next_visible_idx].0);

    display_dialogue(&runtime, &catalog, &cond_ctx, index.0, &mut ui);
}

// ---------------------------------------------------------------------------
// Input: open / advance (X / Space / Enter / left mouse)
// ---------------------------------------------------------------------------

#[derive(SystemParam)]
pub struct InteractInputs<'w, 's> {
    pub player_q: Query<'w, 's, &'static Transform, With<Player>>,
    pub keys: Res<'w, ButtonInput<KeyCode>>,
    pub mouse: Res<'w, ButtonInput<MouseButton>>,
}

pub fn interact(
    inputs: InteractInputs,
    mut game_state: ResMut<GameState>,
    cache: Res<CachedInteractables>,
    mut runtime: ResMut<DialogueRuntime>,
    catalog: Res<DialogueCatalog>,
    mut index: ResMut<DialogueSelectedIndex>,
    mut playback: ResMut<ScenePlayback>,
    cond_ctx: ConditionContext,
    mut effects: EffectDispatcher,
    mut events_dialogue_box: ResMut<Messages<DialogueBoxTriggerEvent>>,
    mut choice_picked: ResMut<Messages<DialogueChoicePickedEvent>>,
    mut ui: DialogueUiParams,
) {
    let open_pressed = inputs.keys.just_pressed(KeyCode::KeyX);
    let advance_pressed = inputs.keys.just_pressed(KeyCode::Space)
        || inputs.keys.just_pressed(KeyCode::Enter)
        || inputs.mouse.just_pressed(MouseButton::Left);

    if !open_pressed && !advance_pressed {
        return;
    }

    match game_state.0 {
        Game_State::Exploring if open_pressed => {
            try_open_dialogue(
                &inputs.player_q,
                &cache,
                &catalog,
                &mut game_state,
                &mut runtime,
                &mut index,
                &mut events_dialogue_box,
            );
        }
        Game_State::Interacting if (open_pressed || advance_pressed) && !playback.blocking() => {
            advance_dialogue(
                &mut runtime,
                &catalog,
                &mut index,
                &mut playback,
                &cond_ctx,
                &mut effects,
                &mut choice_picked,
                &mut game_state,
                &mut ui,
            );
        }
        _ => {}
    }
}

fn try_open_dialogue(
    player_q: &Query<&Transform, With<Player>>,
    cache: &CachedInteractables,
    catalog: &DialogueCatalog,
    game_state: &mut GameState,
    runtime: &mut DialogueRuntime,
    index: &mut DialogueSelectedIndex,
    events_dialogue_box: &mut Messages<DialogueBoxTriggerEvent>,
) {
    for transform in player_q.iter() {
        let player_rect = Rect::from_center_size(
            transform.translation.truncate(),
            Vec2::new(32.0, 32.0),
        );
        let hit = cache.0.iter().find(|(t, _)| {
            let other = Rect::from_center_size(t.translation.truncate(), Vec2::new(32.0, 32.0));
            aabb_collision(player_rect, other)
        });
        if let Some((_, interactable)) = hit {
            if !runtime.start(interactable.dialogue_id.clone(), catalog) {
                continue;
            }
            game_state.0 = Game_State::Interacting;
            index.0 = None;
            events_dialogue_box.write(DialogueBoxTriggerEvent);
            return;
        }
    }
}

fn advance_dialogue(
    runtime: &mut DialogueRuntime,
    catalog: &DialogueCatalog,
    index: &mut DialogueSelectedIndex,
    playback: &mut ScenePlayback,
    cond_ctx: &ConditionContext,
    effects: &mut EffectDispatcher,
    choice_picked: &mut Messages<DialogueChoicePickedEvent>,
    game_state: &mut GameState,
    ui: &mut DialogueUiParams,
) {
    let Some(node) = runtime.current_node(catalog).cloned() else {
        runtime.end();
        game_state.0 = Game_State::Exploring;
        despawn_box(ui);
        return;
    };

    let next_id = match node {
        DialogueNode::Line(LineNode { next, .. }) => next,
        DialogueNode::Choice(ChoiceNode { options, .. }) => {
            let visible = visible_options(&options, cond_ctx);
            let Some(selected_visible) = find_visible_index(&visible, index.0) else {
                return; // require a selection before advancing
            };
            let (_, option) = &visible[selected_visible];
            let option_clone: ChoiceOption = (*option).clone();
            effects.dispatch_all(&option_clone.effects);
            if option_clone.legacy_event_id != 0 {
                choice_picked.write(DialogueChoicePickedEvent {
                    event_id: option_clone.legacy_event_id,
                });
            }
            option_clone.next
        }
        DialogueNode::Scene(SceneNode { .. }) => {
            // Defensive: input shouldn't reach here because `playback.blocking()`
            // gates the call. Treat as no-op.
            return;
        }
    };

    if let Some(scene_id) = effects.pending_scene_change.0.take() {
        runtime.start(scene_id, catalog);
    } else {
        runtime.goto(next_id);
    }
    index.0 = None;

    start_scene_playback_if_needed(runtime, catalog, playback);

    if !runtime.active {
        game_state.0 = Game_State::Exploring;
        despawn_box(ui);
        return;
    }

    display_dialogue(runtime, catalog, cond_ctx, index.0, ui);
}

// ---------------------------------------------------------------------------
// Rendering (text + choices only; portraits live on the stage)
// ---------------------------------------------------------------------------

pub fn redraw_when_runtime_changes(
    runtime: Res<DialogueRuntime>,
    catalog: Res<DialogueCatalog>,
    index: Res<DialogueSelectedIndex>,
    mut last_node: Local<Option<String>>,
    cond_ctx: ConditionContext,
    mut ui: DialogueUiParams,
) {
    let current = runtime.current_node.clone();
    if current == *last_node {
        return;
    }
    *last_node = current;
    if !runtime.active || ui.box_query.iter().next().is_none() {
        return;
    }
    display_dialogue(&runtime, &catalog, &cond_ctx, index.0, &mut ui);
}

fn display_dialogue(
    runtime: &DialogueRuntime,
    catalog: &DialogueCatalog,
    cond_ctx: &ConditionContext,
    selected: Option<usize>,
    ui: &mut DialogueUiParams,
) {
    let Some(node) = runtime.current_node(catalog) else {
        despawn_box(ui);
        return;
    };

    let Ok((box_entity, children)) = ui.box_query.single_mut() else {
        return;
    };

    // Clear existing choice buttons before re-rendering.
    for child in children.iter() {
        if ui.button_query.get(child).is_ok() {
            ui.commands.entity(child).despawn();
        }
    }

    match node {
        DialogueNode::Line(line) => {
            render_text(ui, &line.speaker, &line.text);
        }
        DialogueNode::Choice(choice) => {
            let speaker = choice.prompt.clone().unwrap_or_default();
            let prompt_text = choice.prompt_text.clone().unwrap_or_default();
            render_text(ui, &speaker, &prompt_text);
            render_choice_buttons(ui, box_entity, choice, cond_ctx, selected);
        }
        DialogueNode::Scene(_) => {
            // The scene player owns the screen during a Scene node. Clear
            // any stale dialogue text so the box doesn't show the previous
            // line under the timeline.
            render_text(ui, &Speaker::default(), "");
        }
    }
}

fn render_text(ui: &mut DialogueUiParams, speaker: &Speaker, text: &str) {
    let Ok(text_entity) = ui.text_query.single() else {
        return;
    };
    let label = if speaker.name.trim().is_empty() {
        text.to_string()
    } else {
        format!("{}: {}", speaker.name, text)
    };
    ui.commands.entity(text_entity).insert((
        Text::new(label),
        TextFont {
            font_size: 20.0,
            ..Default::default()
        },
        TextColor(palette::TEXT_PRIMARY),
    ));
}

fn render_choice_buttons(
    ui: &mut DialogueUiParams,
    box_entity: Entity,
    choice: &ChoiceNode,
    cond_ctx: &ConditionContext,
    selected: Option<usize>,
) {
    let visible = visible_options(&choice.options, cond_ctx);
    let selected_visible: Option<usize> =
        selected.and_then(|s| visible.iter().position(|(orig, _)| *orig == s));

    for (visible_idx, (_orig_idx, option)) in visible.iter().enumerate() {
        let is_selected = Some(visible_idx) == selected_visible;
        ui.commands.entity(box_entity).with_children(|parent| {
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
                    ChoiceButton,
                ))
                .with_children(|btn| {
                    btn.spawn((
                        Text::new(&option.text),
                        TextFont {
                            font_size: 17.0,
                            ..Default::default()
                        },
                        TextColor(if is_selected {
                            palette::TEXT_HEADING
                        } else {
                            palette::TEXT_SECONDARY
                        }),
                    ));
                });
        });
    }
}

fn despawn_box(ui: &mut DialogueUiParams) {
    for (box_entity, children) in ui.box_query.iter_mut() {
        for child in children.iter() {
            if ui.button_query.get(child).is_ok() {
                ui.commands.entity(child).despawn();
            }
        }
        ui.commands.entity(box_entity).despawn();
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn visible_options<'a>(
    options: &'a [ChoiceOption],
    cond_ctx: &ConditionContext,
) -> Vec<(usize, &'a ChoiceOption)> {
    options
        .iter()
        .enumerate()
        .filter(|(_, opt)| {
            opt.condition
                .as_ref()
                .map(|c| evaluate_condition(c, cond_ctx))
                .unwrap_or(true)
        })
        .collect()
}

fn find_visible_index(
    visible: &[(usize, &ChoiceOption)],
    selected_orig: Option<usize>,
) -> Option<usize> {
    let s = selected_orig?;
    visible.iter().position(|(orig, _)| *orig == s)
}
