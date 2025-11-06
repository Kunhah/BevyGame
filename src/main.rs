use bevy::core_pipeline::core_2d::graph::input;
use bevy::ecs::{entity, query};
use bevy::math::ops::powf;
use bevy::prelude::*;
use bevy::render::camera;
use bevy::render::view::RenderLayers;
use bevy::ui::prelude::*;
use bevy::ecs::event::Events;
use bevy::ui::{PositionType};
use bevy::window::{WindowMode, MonitorSelection, VideoModeSelection};
use bevy::sprite::*;
use bevy::prelude::GltfAssetLabel::Texture;
use bevy::input::keyboard::KeyCode; // KeyCode fix
use bevy::math::Rect;
use bevy::state::commands;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::RwLock;
use std::sync::RwLockReadGuard;
use std::sync::RwLockWriteGuard;
use once_cell::sync::Lazy;
use bevy::prelude::Circle;
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;
use std::hash::Hasher;
use std::cmp::Ordering;
use approx::AbsDiffEq;
use serde::Deserialize;
use serde_json::*;
use std::fs::File;
use std::io::BufReader;
use bitflags::bitflags;
use noise::{NoiseFn, Perlin};

mod light_plugin;
use light_plugin::*;

const SEGMENT_SIZE: f32 = 8.0;
const LIGHT_ALPHA: f32 = 0.15;
const LIGHT_WIDTH: f32 = 2.0;
const NOISE_SCALE: f64 = 0.2;
const MAX_DISTANCE_LIGHT: f32 = 200.0;

const MAX_DISTANCE_RENDER: f32 = 1000.0;
const MAX_DISTANCE_COLLISION: f32 = 10.0;

const MAX_OBJECTS: usize = 4;
const MAX_LEVELS: usize = 5;

const WINDOW_WIDTH: f32 = 1920.0;
const WINDOW_HEIGHT: f32 = 1080.0;
const PLAYER_SPEED: f32 = 200.0;

const GRID_WIDTH: u32 = 15000;
const GRID_HEIGHT: u32 = 15000;

const PATH_MARGIN: i32 = 5;
const PATH_DRAW_MARGIN: i32 = 4;
const PATH_MOVEMENT_SPEED: u32 = 20;

const WALKING_LIMIT: usize = 600/PATH_DRAW_MARGIN as usize;

bitflags! {
    struct Flags: u128 {
        const FLAG1 = 1 << 0; // 0000 0001
        const FLAG2 = 1 << 1; // 0000 0010
        const FLAG3 = 1 << 2; // 0000 0100
        const FLAG4 = 1 << 3; // 0000 1000
    }
}

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
enum DialogueSet {
    Spawn,
    Interact,
}

#[derive(Debug, Deserialize)]
pub struct Choice {
    pub event: u32,
    pub text: String,
    pub next: Option<String>,
}

impl Default for Choice {
    fn default() -> Self {
        Choice {
            event: 0,
            text: String::new(),
            next: None,
        }
    }
}

impl Clone for Choice {
    fn clone(&self) -> Self {
        Choice {
            event: self.event.clone(),
            text: self.text.clone(),
            next: self.next.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct DialogueLine {
    pub id: String,
    pub speaker: String,
    pub text: String,
    pub next: Option<String>,
    pub choices: Option<Vec<Choice>>,
}

#[derive(Debug, Clone, Copy)]
enum GameState {
    Exploring,
    Interacting,
    Battle
}

pub struct DialogueData(pub HashMap<String, DialogueLine>);

impl Default for DialogueData {
    fn default() -> Self {
        DialogueData(HashMap::new())
    }
}

pub struct DialogueState {
    pub current_id: Option<String>,
    pub active: bool,
    pub just_spawned: bool
}

impl Default for DialogueState {
    fn default() -> Self {
        DialogueState {
            current_id: None,
            active: false,
            just_spawned: false
        }
    }
}

impl Default for GameState {
    fn default() -> Self {
        GameState::Exploring
    }
}

struct GlobalVariables {
    moving: bool,
    camera_locked: bool
}

impl Default for GlobalVariables {
    fn default() -> Self {
        GlobalVariables { 
            moving: false,
            camera_locked: false
        }
    }
}

// New component to mark walls
#[derive(Component, Clone)]
struct Collider{
    pub bounds: Rect,
}

pub struct QuadtreeNode {
    pub bounds: Rect,
    pub level: usize,
    pub objects: Vec<Collider>,
    pub children: Option<[Box<QuadtreeNode>; 4]>,
}

#[derive(Component, Clone)]
struct Interactable {
    name: String,
    dialogue_id: String,
}

#[derive(Component)]
struct MainCamera;

impl Interactable {
    fn interact(
        &self, transform: &bevy::prelude::Transform,
        mut game_state: GameState,
        //mut commands: Commands,
        mut state: ResMut<Dialogue_State>,
        //data: Res<Dialogue_Data>,
        //asset_server: Res<AssetServer>,
        //mut box_query: Query<(Entity, &Children), With<DialogueBox>>,
        //text_query: Query<Entity, With<DialogueText>>,
        //button_query: Query<Entity, With<ChoiceButton>>,
        //mut dialogue_trigger: ResMut<DialogueTrigger>,
        mut events_dialogue_box: ResMut<Events<DialogueBoxTriggerEvent>>,
    ) {
        // code to handle interaction goes here
        //let previous_state = game_state.clone();
        game_state = GameState::Interacting;
        println!("Interacting, game state: {:?}", game_state);
        state.0.current_id = Some(self.dialogue_id.clone());
        state.0.active = true;
        println!("Triggered");
        //dialogue_trigger.0 = true;
        let event = DialogueBoxTriggerEvent {};
        events_dialogue_box.send(event);
    }
}

#[derive(Component)]
struct FadeOutTimer(Timer);

fn fade_out_system(mut commands: Commands, time: Res<Time>, mut query: Query<(Entity, &mut FadeOutTimer, &mut Sprite)>) {
    for (entity, mut timer, mut sprite) in query.iter_mut() {
        timer.0.tick(time.delta());
        if timer.0.just_finished() {
            commands.entity(entity).despawn();
        } else {
            let r = sprite.color.to_srgba().red;
            let g = sprite.color.to_srgba().green;
            let b = sprite.color.to_srgba().blue;
            let a = sprite.color.alpha();
            let new_alpha = (a - 0.01).max(0.0); // Ensure alpha does not go below 0.0
            sprite.color = Color::srgba(r, g, b, new_alpha);
        }
    }
}

#[derive(Component)]
struct AnimationIndices {
    first: usize,
    last: usize,
}

#[derive(Component, Deref, DerefMut)]
struct AnimationTimer(Timer);

fn animate_sprite(
    time: Res<Time>,
    mut query: Query<(&AnimationIndices, &mut AnimationTimer, &mut Sprite)>,
) {
    for (indices, mut timer, mut sprite) in &mut query {
        timer.tick(time.delta());

        if timer.just_finished() {
            if let Some(atlas) = &mut sprite.texture_atlas {
                atlas.index = if atlas.index == indices.last {
                    indices.first
                } else {
                    atlas.index + 1
                };
            }
        }
    }
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest()).set(WindowPlugin {
            primary_window: Some(Window {
                title: "Seirei Kuni".to_string(),
                resolution: (WINDOW_WIDTH, WINDOW_HEIGHT).into(),
                //mode: WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Current),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest()))
        .add_plugins(LightPlugin) // ✅ your plugin
        .insert_resource(ClearColor(Color::srgb(0.1, 0.1, 0.1)))
        .insert_resource(CachedInteractables(Vec::new()))
        .insert_resource(CachedColliders(Vec::new()))
        .insert_resource(Game_State(GameState::Exploring))
        .insert_resource(Global_Variables(GlobalVariables::default()))
        .insert_resource(Dialogue_State(DialogueState::default()))
        .insert_resource(Selected_Choice(Choice::default()))
        .insert_resource(Selected_Choice_Index(0))
        //.insert_resource(DialogueTrigger(false))
        //.insert_resource(DialogueJustSpawned(false))
        .insert_resource(Next_Id(HashMap::new()))
        .insert_resource(Conditionals(Flags::empty()))
        .insert_resource(Events::<DialogueBoxTriggerEvent>::default())
        .insert_resource(Events::<DialogueTriggerEvent>::default())
        .insert_resource(DayCycle(480)) // every unit is 1 minute, 480 is equal to 08:00
        .add_systems(Startup, setup)
        .add_systems(Update, player_movement)
        .add_systems(Update, toggle_camera_lock)
        .add_systems(Update, update_cache)
        .add_systems(Update, mouse_click)
        .add_systems(Update, follow_path_system)
        .add_systems(Update, enter_battle)
        .add_systems(Update, spawn_dialogue_box.in_set(DialogueSet::Spawn))
        .add_systems(Update, interact.in_set(DialogueSet::Interact).after(DialogueSet::Spawn))
        .add_systems(Update, create_first_dialogue)
        .add_systems(Update, gui_selection)
        //.add_systems(Update, update_light_params)
        .run();
}

fn create_first_dialogue(
    mut commands: Commands,
    mut state: ResMut<Dialogue_State>,
    data: Res<Dialogue_Data>,
    box_query: Query<(Entity, &Children), With<DialogueBox>>,
    text_query: Query<Entity, With<DialogueText>>,
    button_query: Query<Entity, With<ChoiceButton>>,
    mut index: ResMut<Selected_Choice_Index>,
    mut selected: ResMut<Selected_Choice>,
    mut events_dialogue: ResMut<Events<DialogueTriggerEvent>>,
) {
    for _event in events_dialogue.drain() {
        println!("Spawing first dialogue");
        state.0.just_spawned = true; // Set flag
    }

    // Run dialogue display in the next frame
    if state.0.just_spawned {
        if box_query.iter().next().is_some() {
            display_dialogue(&mut commands, &mut state, &data, box_query, text_query, button_query, &mut index, &mut selected);
            state.0.just_spawned = false;
        }
    }
}

#[derive(Component)]
struct Player;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Component)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(Resource, Default)]
struct Game_State(GameState);

#[derive(Resource, Default)]
struct Global_Variables(GlobalVariables);

#[derive(Resource, Default)]
struct CachedInteractables(Vec<(Transform, Interactable)>);

#[derive(Resource, Default)]
struct QuadTree(QuadtreeNode);

#[derive(Resource)]
pub struct DayCycle(u32);
// It is scaled by << 4 (* 16), to be directly compatible with duration of effects, to display the time in hors and minutes:
// temp_x = x >> 4
// h = temp_x / 60
// m = temp_x % 60

#[derive(Resource, Default)]
pub struct CachedColliders(Vec<(Transform, Collider)>);

#[derive(Resource, Default)]
pub struct Dialogue_State(DialogueState);

#[derive(Resource, Default)]
pub struct Dialogue_Data(pub HashMap<String, DialogueLine>);

#[derive(Resource, Default)]
pub struct Selected_Choice(Choice);

#[derive(Resource, Default)]
pub struct Selected_Choice_Index(u8);

#[derive(Resource, Default)]
pub struct Next_Id(pub HashMap<String, String>);

#[derive(Resource)]
pub struct Conditionals(Flags);

#[derive(Component)]
struct MoveAlongPath {
    path: Vec<IVec2>,
    current_index: usize,
    timer: Timer,
}

#[derive(Component)]
struct DialogueText;

#[derive(Component)]
struct ChoiceButton {
    next_id: String,
}

#[derive(Component)]
struct DialogueBox;

/* #[derive(Resource)]
struct DialogueTrigger(bool);

#[derive(Resource, Default)]
struct DialogueJustSpawned(bool); */

#[derive(Event)]
struct DialogueBoxTriggerEvent {
}

#[derive(Event)]
struct DialogueTriggerEvent {
}

fn spawn_dialogue_box(
    mut commands: Commands,
    mut events_dialogue_box: ResMut<Events<DialogueBoxTriggerEvent>>,
    mut events_dialogue: ResMut<Events<DialogueTriggerEvent>>,
) {
    for event in events_dialogue_box.drain() {
        println!("Function called");
        commands
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0), // Full screen container
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::End, // Align to bottom
                    align_items: AlignItems::Center,      // Center horizontally
                    padding: UiRect::all(Val::Px(0.0)),   // No padding on container
                    ..default()
                },
                BackgroundColor(Color::NONE), // Transparent background
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
        //trigger.0 = false;
        println!("Dialogue box spawned");
        let event = DialogueTriggerEvent {};
        events_dialogue.send(event);
        //just_spawned.0 = true;
        println!("Spawn message sent");
    }
}

fn setup(
    mut commands: Commands, 
    asset_server: Res<AssetServer>,
    query: Query<(Entity, &Transform), With<Collider>>,
) {

    let mut quadtree = QuadtreeNode::new(
        Rect::from_center_size(Vec2::ZERO, Vec2::splat(2048.0)),
        0,
    );

    // Insert all colliders into the quadtree
    for (_, transform) in &query {
        let pos = transform.translation.truncate();
        let rect = Rect::from_center_size(pos, Vec2::splat(32.0));

        quadtree.insert(Collider {
            bounds: rect,
        });
    }

    // Save the quadtree as a Bevy resource
    commands.insert_resource(QuadTree(quadtree));

    let dialogue_data = load_dialogue();
    commands.insert_resource(Dialogue_Data(dialogue_data));
    
    commands
    .spawn(Camera2d::default())
    .insert(MainCamera)
    .insert(Transform::from_xyz(0.0, 0.0, 0.0))
    .insert(Position { x: 0, y: 0 });

    commands.spawn((
        Sprite {
            image: asset_server.load("character.png"),
            custom_size: Some(Vec2::new(32.0, 32.0)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0), 
        Position { x: 0, y: 0 },
        Player,
    ));

    // Spawn some walls
    for x in 5..7 {
        commands.spawn((
            Sprite {
                image: asset_server.load("character.png"),
                color: Color::srgb(0.8, 0.1, 0.1),
                custom_size: Some(Vec2::new(32.0, 32.0)),
                ..default()
            },
            Transform::from_xyz(x as f32 * 32.0, 5.0 * 32.0, 0.0),
            Position { x: x * 32, y: 5 * 32 },
            Collider { bounds: Rect::from_center_size(Vec2::new(x as f32 * 32.0, 5.0 * 32.0), Vec2::splat(32.0)) },
            Visibility::Visible,
            InheritedVisibility::default(),
            ViewVisibility::default(),
            RenderLayers::layer(1),                  // only the occlusion camera sees it
            Occluder,
        ));
    }

    for x in 1..3 {
        commands.spawn((
            Sprite {
                image: asset_server.load("character.png"),
                color: Color::srgb(0.8, 0.1, 0.1),
                custom_size: Some(Vec2::new(32.0, 32.0)),
                ..default()
            },
            Transform::from_xyz(x as f32 * 32.0, 5.0 * 32.0, 0.0),
            Position { x: x * 32, y: 5 * 32 },
            Interactable { name: "Test interactable".to_string(), dialogue_id: "The last goodbye 1".to_string() },
        ));
    }
}

fn player_movement(

    mut param_set: ParamSet<(
        Query<(&mut Transform, &mut Position), With<Player>>,
        Query<(&mut Transform, &mut Position), With<MainCamera>>,
        ResMut<Global_Variables>,
    )>,
    //mut camera: Query<(&mut Transform, &mut Position), With<MainCamera>>,
    game_state: Res<Game_State>,
    cache: Res<CachedColliders>,
    quad_tree: Res<QuadTree>,
    input: Res<ButtonInput<KeyCode>>, 
    time: Res<Time>,
    mut commands: Commands,
    //query: Query<(Entity, &LightSegment)>
    //mut index: ResMut<Selected_Choice_Index>,

) {
    let mut direction = Vec2::ZERO;

    if input.pressed(KeyCode::KeyW) {
        direction.y += 1.0;
    }
    if input.pressed(KeyCode::KeyS) {
        direction.y -= 1.0;
    }
    if input.pressed(KeyCode::KeyD) {
        direction.x += 1.0;
    }
    if input.pressed(KeyCode::KeyA) {
        direction.x -= 1.0;
    }

    let movement_speed = PLAYER_SPEED * time.delta_secs();

    let camera_locked = param_set.p2().0.camera_locked;

    let camera_position = param_set.p1().single().unwrap().0.translation.truncate();

    if direction.length() > 0.0 {
        match game_state.0 {
            GameState::Exploring => {
                let mut new_x_out: Option<f32> = None;
                let mut new_y_out: Option<f32> = None;

                if direction.x != 0.0 && direction.y != 0.0 {

                    let diagonal_speed = movement_speed / (2.0_f32.sqrt());
        
                    let mut p0 = param_set.p0();
        
                    for (mut transform, mut position) in p0.iter_mut() {
                        let new_x = transform.translation.x + direction.x * diagonal_speed;
                        let new_y = transform.translation.y + direction.y * diagonal_speed;

                        new_x_out = Some(new_x);
                        new_y_out = Some(new_y);

                        transform.rotation = Quat::from_rotation_z(
                            rotate_to_direction(transform.translation.x, transform.translation.y, new_x, new_y),
                        );
        
                        if ((new_x.abs() as u32) < GRID_WIDTH) && ((new_y.abs() as u32) < GRID_HEIGHT) {
                            
                            // let collision = cache.0.iter().any(|wall_transform| {
                            //     let player_vec2 = Vec2::new(new_x, new_y);
                            //     let wall_vec2 = Vec2::new(wall_transform.0.translation.x, wall_transform.0.translation.y);
                            //     if wall_vec2.distance_squared(player_vec2) > MAX_DISTANCE_COLLISION {
                            //         return false;
                            //     }
                            //     let player_rect = Rect::from_center_size(player_vec2, Vec2::new(32.0, 32.0));
                            //     let wall_rect = Rect::from_center_size(
                            //         wall_vec2,
                            //         Vec2::new(32.0, 32.0),
                            //     );
                            //     aabb_collision(player_rect, wall_rect)
                            // });
                            let new_pos = Position{x: new_x as i32, y: new_y as i32};
                            //let collision = is_walkable(new_pos, &quad_tree);
        
                            if is_walkable_move(new_pos, &quad_tree) {
                                transform.translation.x = new_x;
                                transform.translation.y = new_y;
                                position.x = new_x as i32;
                                position.y = new_y as i32;

                                let player_vec2 = Vec2::new(new_x, new_y);
                                // FUNCTIONS TO UPDATE THE LIGHT GOES HERE:
                                // cleanup_light_segments(&mut commands, query);
                                // emit_light(&mut commands, &time, player_vec2, camera_position, &quad_tree);

                            }
                        }
                    }
                } else {

                    for (mut transform, mut position) in param_set.p0().iter_mut() {
                        let new_x = transform.translation.x + direction.x * movement_speed;
                        let new_y = transform.translation.y + direction.y * movement_speed;

                        new_x_out = Some(new_x);
                        new_y_out = Some(new_y);
        
                        transform.rotation = Quat::from_rotation_z(
                            rotate_to_direction(transform.translation.x, transform.translation.y, new_x, new_y),
                        );
        
                        if ((new_x.abs() as u32) < GRID_WIDTH) && ((new_y.abs() as u32) < GRID_HEIGHT) {
                            
                            // let collision = cache.0.iter().any(|wall_transform| {
                            //     let player_vec2 = Vec2::new(new_x, new_y);
                            //     let wall_vec2 = Vec2::new(wall_transform.0.translation.x, wall_transform.0.translation.y);
                            //     if wall_vec2.distance_squared(player_vec2) > MAX_DISTANCE_COLLISION {
                            //         return false;
                            //     }
                            //     let player_rect = Rect::from_center_size(player_vec2, Vec2::new(32.0, 32.0));
                            //     let wall_rect = Rect::from_center_size(
                            //         wall_vec2,
                            //         Vec2::new(32.0, 32.0),
                            //     );
                            //     aabb_collision(player_rect, wall_rect)
                            // });
                            let new_pos = Position{x: new_x as i32, y: new_y as i32};
                            //let collision = is_walkable(new_pos, &quad_tree);
        
                            if is_walkable_move(new_pos, &quad_tree) {
                                transform.translation.x = new_x;
                                transform.translation.y = new_y;
                                position.x = new_x as i32;
                                position.y = new_y as i32;

                                let player_vec2 = Vec2::new(new_x, new_y);
                                // FUNCTIONS TO UPDATE THE LIGHT GOES HERE:
                                // cleanup_light_segments(&mut commands, query);
                                // emit_light(&mut commands, &time, player_vec2, camera_position, &quad_tree);

                            }
                        }
                    }
                }
                if camera_locked && (new_x_out.is_some() || new_y_out.is_some()) {
                    println!("moving camera");
                    let new_x = new_x_out.unwrap();
                    let new_y = new_y_out.unwrap();
                    for (mut transform_c, mut position_c) in param_set.p1().iter_mut() {
                        transform_c.translation.x = new_x;
                        transform_c.translation.y = new_y;
                        position_c.x = new_x as i32;
                        position_c.y = new_y as i32;
                    }
                }
            }
            GameState::Battle => {
            }
            GameState::Interacting => {
            }
        }
        
    }
}

fn gui_selection(
    input: Res<ButtonInput<KeyCode>>, 
    game_state: Res<Game_State>,
    mut index: ResMut<Selected_Choice_Index>,
    mut commands: Commands,
    mut state: ResMut<Dialogue_State>,
    data: Res<Dialogue_Data>,
    asset_server: Res<AssetServer>,
    mut box_query: Query<(Entity, &Children), With<DialogueBox>>,
    text_query: Query<Entity, With<DialogueText>>,
    button_query: Query<Entity, With<ChoiceButton>>,
    mut selected: ResMut<Selected_Choice>,
) {
    if input.just_pressed(KeyCode::KeyW) || input.just_pressed(KeyCode::KeyS) || input.just_pressed(KeyCode::KeyA) || input.just_pressed(KeyCode::KeyD) {
        let vertical = input.just_pressed(KeyCode::KeyS) as i8 - input.just_pressed(KeyCode::KeyW) as i8;
        let horizontal = input.just_pressed(KeyCode::KeyD) as i8 - input.just_pressed(KeyCode::KeyA) as i8;
        println!("Vertical: {}, Horizontal: {}", vertical, horizontal);
        match game_state.0 {
            GameState::Exploring => {
            }
            GameState::Battle => {
            }
            GameState::Interacting => {
                index.0 = index.0.wrapping_add(vertical as u8);
                    println!("index: {}", index.0);
                    display_dialogue(&mut commands, &mut state, &data, box_query, text_query, button_query, &mut index, &mut selected);
            }
        }
    }

}

fn interact<'a>(
    mut param_set: ParamSet<(
        Query<(&Transform, &Position), With<Player>>,
        Res<ButtonInput<KeyCode>>, 
    )>,
    //input: Res<ButtonInput<KeyCode>>, 
    mut game_state: ResMut<Game_State>,
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

            GameState::Exploring => {

                let p0 = param_set.p0();
            
                for (transform, position) in p0.iter() {
            
                    let player_rect = Rect::from_center_size(Vec2::new(transform.translation.x, transform.translation.y), Vec2::new(32.0, 32.0));
            
                    let interactable: Option<&(bevy::prelude::Transform, Interactable)> = cache.0.iter().find(|(interactable_transform, interactable)| {
                        let wall_rect = Rect::from_center_size(
                            Vec2::new(interactable_transform.translation.x, interactable_transform.translation.y),
                            Vec2::new(32.0, 32.0),
                        );
                        aabb_collision(player_rect, wall_rect)
                    });
            
                    if let Some((interactable_transform, interactable)) = interactable {
                        game_state.0 = GameState::Interacting;
                        dialogue_state.0.active = true;
                        interactable.interact(interactable_transform, game_state.0, dialogue_state, events_dialogue_box);
                        break;
                    }
                }
            }
            GameState::Battle => {
                
            }
            GameState::Interacting => {

            }
            _ => {}
        }
    }
    else if param_set.p1().just_pressed(KeyCode::Space) || param_set.p1().just_pressed(KeyCode::Enter) {

        match game_state.0 {

            GameState::Exploring => {

            }
            GameState::Battle => {
                
            }
            GameState::Interacting => {
                println!("Space pressed when interacting");
                if let Some(current_id) = &dialogue_state.0.current_id {
                    if let Some(line) = dialogue_data.0.get(current_id) {

                        let current_choices = &line.choices;
                        if current_choices.is_some() {
                            if selected_choice.0.next.is_none() {
                                return;
                            }
                            dialogue_state.0.current_id = handle_next_id(selected_choice.0.next.clone(), &next_id_map);
                            handle_choice_event(selected_choice.0.event, next_id_map, conditionals);
                        }
                        else {
                            dialogue_state.0.current_id = handle_next_id(line.next.clone(), &next_id_map);
                            if dialogue_state.0.current_id.is_none() {
                                dialogue_state.0.active = false;
                                game_state.0 = GameState::Exploring;
                            }
                        }
                        display_dialogue(&mut commands, &mut dialogue_state, &dialogue_data, box_query, text_query, button_query, &mut index, &mut selected_choice);
                    }
                }
            }
            _ => {}
        }
    }
}

fn aabb_collision(rect1: Rect, rect2: Rect) -> bool {
    rect1.min.x < rect2.max.x &&
    rect1.max.x > rect2.min.x &&
    rect1.min.y < rect2.max.y &&
    rect1.max.y > rect2.min.y
}

fn update_interactable_cache(
    mut cache: ResMut<CachedInteractables>,
    query: Query<(&Transform, &Interactable), With<Interactable>>,
) {
    cache.0 = query
        .iter()
        .map(|(t, i)| (t.clone(), i.clone()))
        .collect();
}

// fn update_collider_cache(
//     mut cache: ResMut<CachedColliders>,
//     //query: Query<(&Transform, &Collider), With<Collider>>,
//     quadTree: Res<QuadTree>,
// ) {
//     cache.0 = query
//         .iter()
//         .map(|(t, i)| (t.clone(), i.clone()))
//         .collect();
// }

fn update_quad_tree(
    //mut cache: ResMut<CachedColliders>,
    query: Query<(Entity, &Transform), With<Collider>>,
    mut quadTree: ResMut<QuadTree>,
) {
    let mut quadtree = QuadtreeNode::new(
        Rect::from_center_size(Vec2::ZERO, Vec2::splat(2048.0)),
        0,
    );

    // Insert all colliders into the quadtree
    for (_, transform) in &query {
        let pos = transform.translation.truncate();
        let rect = Rect::from_center_size(pos, Vec2::splat(32.0));

        quadtree.insert(Collider {
            bounds: rect,
        });
    }

    quadTree.0 = quadtree;
}


fn update_cache(
    cache_interactables: ResMut<CachedInteractables>,
    //cache_colliders: ResMut<CachedColliders>,
    interactable_query: Query<(&Transform, &Interactable), With<Interactable>>,
    query: Query<(Entity, &Transform), With<Collider>>,
    mut quadTree: ResMut<QuadTree>,
    //collider_query: Query<(&Transform, &Collider), With<Collider>>,
    //query: Query<(Entity, &Transform)>,
    input: Res<ButtonInput<KeyCode>>, 
) {
    let mut quadtree = QuadtreeNode::new(Rect::from_center_size(Vec2::ZERO, Vec2::splat(2048.0)), 0);

    // for (entity, transform) in &query {
    //     let pos = transform.translation.truncate();
    //     let rect = Rect::from_center_size(pos, Vec2::splat(32.0));
    //     quadtree.insert(Collider { bounds: rect });
    // }


    if(input.just_pressed(KeyCode::KeyP)) {
        update_interactable_cache(cache_interactables, interactable_query);
        update_quad_tree(query, quadTree);
        //update_collider_cache(cache_colliders, collider_query);
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct Node_P {
    position: Position,
    cost: i32,         // f = g + h
    priority: i32,     // used for ordering
}

impl Ord for Node_P {
    fn cmp(&self, other: &Self) -> Ordering {
        other.priority.cmp(&self.priority) // reverse for min-heap
            .then_with(|| other.cost.cmp(&self.cost))
    }
}

impl PartialOrd for Node_P {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(other.priority.cmp(&self.priority) // reverse for min-heap
        .then_with(|| other.cost.cmp(&self.cost)))
    }
}

fn distance(a: Position, b: Position) -> i32 {
    (10.0 * ((powf((a.x - b.x).abs() as f32, 2.0) + powf((a.y - b.y).abs() as f32, 2.0)).sqrt())).round() as i32
}

// fn distance_vec2(a: &Vec2, b: &Vec2) -> f32 {
//     10.0 * ((powf((a.x - b.x).abs(), 2.0) + powf((a.y - b.y).abs(), 2.0)))
// }

fn is_walkable_move(pos: Position, quad_tree: &QuadTree) -> bool { // Test it without quadtree to see diference in performance

    if pos.x.abs() as u32 > GRID_WIDTH || pos.y.abs() as u32 > GRID_HEIGHT {
        return false;
    }

    let pos_center = Vec2::new(pos.x as f32, pos.y as f32);
    let player_rect = Rect::from_center_size(pos_center, Vec2::new(32.0, 32.0));

    let mut possible_colliders: Vec<&Collider> = Vec::new();
    quad_tree.0.query(player_rect, &mut possible_colliders);

    !possible_colliders.iter().any(|collider| {
        aabb_collision(player_rect, collider.bounds)
    })
}

fn is_walkable_path(pos: Position, quad_tree: &QuadTree) -> bool { // Use a different algorithm since it is something that is moving. Maybe I will, maybe not

    if pos.x.abs() as u32 > GRID_WIDTH || pos.y.abs() as u32 > GRID_HEIGHT {
        return false;
    }

    let pos_center = Vec2::new(pos.x as f32, pos.y as f32);
    let player_rect = Rect::from_center_size(pos_center, Vec2::new(32.0, 32.0));

    let mut possible_colliders: Vec<&Collider> = Vec::new();
    quad_tree.0.query(player_rect, &mut possible_colliders);

    !possible_colliders.iter().any(|collider| {
        aabb_collision(player_rect, collider.bounds)
    })
}
pub fn pathfinding(
    //cache: Res<CachedColliders>,
    quad_tree: Res<QuadTree>,
    start: Position,
    goal: Position,
    margin: i32
) -> Vec<Position> {

    if !is_walkable_path(start, &quad_tree) || !is_walkable_path(goal, &quad_tree) {
        return Vec::new();
    }

    let mut open_set = BinaryHeap::new();
    open_set.push(Node_P {
        position: start,
        cost: 0,
        priority: distance(start, goal),
    });

    let mut next_Node_P: Node_P = Node_P {
        position: start,
        cost: 0,
        priority: distance(start, goal),
    };

    let mut came_from: HashMap<Position, Position> = HashMap::new();
    let mut g_score: HashMap<Position, i32> = HashMap::new();
    g_score.insert(start, 0);

    let mut visited: HashSet<Position> = HashSet::new();

    while !((next_Node_P.position.x - goal.x).abs() < margin && (next_Node_P.position.y - goal.y).abs() < margin) {

        if visited.contains(&next_Node_P.position) {
            next_Node_P = open_set.pop().unwrap();
            continue;
        }
        if visited.len() > 1000 {
            let mut previou_Node_P_position = next_Node_P.position;
            while previou_Node_P_position != start {
                println!("Previous Node_P: {:#?}", previou_Node_P_position);
                previou_Node_P_position = came_from.get(&previou_Node_P_position).unwrap().clone();
            }
            break;
        }
        visited.insert(next_Node_P.position);

        let neighbors = [
            Position { x: next_Node_P.position.x + margin, y: next_Node_P.position.y - margin },
            Position { x: next_Node_P.position.x + margin, y: next_Node_P.position.y },
            Position { x: next_Node_P.position.x + margin, y: next_Node_P.position.y + margin },
            Position { x: next_Node_P.position.x, y: next_Node_P.position.y + margin },
            Position { x: next_Node_P.position.x, y: next_Node_P.position.y - margin },
            Position { x: next_Node_P.position.x - margin, y: next_Node_P.position.y - margin },
            Position { x: next_Node_P.position.x - margin, y: next_Node_P.position.y },
            Position { x: next_Node_P.position.x - margin, y: next_Node_P.position.y + margin },
        ];


        for neighbor in neighbors {

            if !is_walkable_path(neighbor, &quad_tree) {
                println!("Skipped neighbor collider: ({}, {})", neighbor.x, neighbor.y);
                continue;
            }

            let movement_cost = if neighbor.x == next_Node_P.position.x || neighbor.y == next_Node_P.position.y {
                10
            } else {
                14
            };

            let tentative_g = g_score.get(&next_Node_P.position).unwrap_or(&i32::MAX) + movement_cost;

            if tentative_g < *g_score.get(&neighbor).unwrap_or(&i32::MAX) {

                came_from.insert(neighbor, next_Node_P.position);
                g_score.insert(neighbor, tentative_g);

                open_set.push(Node_P {
                    position: neighbor,
                    cost: tentative_g,
                    priority: tentative_g + distance(neighbor, goal),
                });
            }
            else {

            }
        }
        let old_Node_P = next_Node_P;

        next_Node_P = open_set.pop().unwrap();

        if next_Node_P == old_Node_P {
            for Node_P in open_set.iter() {
            }
            println!("Failed to find path");
            break;
        }
    }
    println!("Reached the end of the while loop");

    let mut path = vec![next_Node_P.position];
    let mut curr = next_Node_P.position;
    while let Some(&prev) = came_from.get(&curr) {
        path.push(prev);
        curr = prev;
    }
    path.reverse();

    return path;
}

fn rotate_to_direction(
    start_x: f32,
    start_y: f32,
    destination_x: f32,
    destination_y: f32
) -> f32 {
    let dx = destination_x - start_x;
    let dy = destination_y - start_y;
    let angle = (dy as f32).atan2(dx as f32);
    angle
}

fn follow_path_system(
    mut commands: Commands,
    mut query: Query<(&mut Transform, &mut Position, &mut MoveAlongPath, Entity), Without<MainCamera>>,
    camera_query: Query<(&Transform, &Position), With<MainCamera>>,
    quad_tree: Res<QuadTree>,
    time: Res<Time>,
    // light_query: Query<Entity, &LightSegment>>,
    mut global_variables: ResMut<Global_Variables>,
) {
    let camera_position = camera_query.single().unwrap().0.translation.truncate();
    global_variables.0.moving = true;
    for (mut transform, mut position, mut movement, entity) in query.iter_mut() {
        //movement.timer = Timer::from_seconds(0.1, TimerMode::Once);
        if movement.timer.tick(time.delta() * PATH_MOVEMENT_SPEED).just_finished() {
            if movement.current_index < movement.path.len() {

                let next_tile = movement.path[movement.current_index];
                let target_x = next_tile.x as f32;
                let target_y = next_tile.y as f32;

                transform.rotation = Quat::from_rotation_z(
                    rotate_to_direction(transform.translation.x, transform.translation.y, target_x, target_y),
                );
                transform.translation.x = target_x;
                transform.translation.y = target_y;
                position.x = next_tile.x;
                position.y = next_tile.y;

                movement.current_index += 1;

                // FUNCTIONS TO UPDATE THE LIGHT GOES HERE:
                // cleanup_light_segments(&mut commands, light_query);
                // emit_light(&mut commands, &time, transform.translation.truncate(), camera_position, &quad_tree);

            } else {
                commands.entity(entity).remove::<MoveAlongPath>();
            }
        }
    }
    global_variables.0.moving = false;
}

fn toggle_camera_lock(
    mut param_set: ParamSet<(
        Query<(&mut Transform, &mut Position), With<Player>>,
        Query<(&mut Transform, &mut Position), With<MainCamera>>,
        ResMut<Global_Variables>,
    )>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if input.just_pressed(KeyCode::KeyL) {

        //param_set.p2().0.camera_locked = !param_set.p2().0.camera_locked;     
        if !param_set.p2().0.camera_locked {
            param_set.p2().0.camera_locked = true;

            let mut position_x: i32 = 0;
            let mut position_y: i32 = 0;
            let mut transform_x: f32 = 0.0;
            let mut transform_y: f32 = 0.0;

            for (player_transform, player_position) in param_set.p0().iter_mut() {
                position_x = player_position.x;
                position_y = player_position.y;
                transform_x = player_transform.translation.x;
                transform_y = player_transform.translation.y;
            }

            for (mut camera_transform, mut camera_position) in param_set.p1().iter_mut() {
                camera_position.x = position_x;
                camera_position.y = position_y;
                camera_transform.translation.x = transform_x;
                camera_transform.translation.y = transform_y;
            }
        }  
        else {
            param_set.p2().0.camera_locked = false;
        }
        println!("Camera locked: {}", param_set.p2().0.camera_locked); 
    }
}

fn mouse_click(
    mut param_set: ParamSet<(
        Query<(Entity, &mut Transform, &mut Position), With<Player>>,
    )>,
    game_state: Res<Game_State>,
    camera_query: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    //cache: Res<CachedColliders>,
    quad_tree: Res<QuadTree>,
    input: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    time: Res<Time>
) {
    if input.just_pressed(MouseButton::Left) {

        let mut p0 = param_set.p0();
        let (entity, mut transform, mut position) = p0.iter_mut().next().unwrap();

        let player_entity = entity;
        
        let path_ops = find_path(*position, game_state.0, quad_tree, camera_query, windows, PATH_DRAW_MARGIN);
        if path_ops.is_none() {
            return;
        }
        let path = path_ops.unwrap();
        if path.is_empty() {
            return;
        }
        let path_len = path.len();
        if path_len > WALKING_LIMIT {
            println!("Path too long");
            return;
        }

        println!("path len: {}", path_len);

        if path_len > 1 {
            let path_iv2: Vec<IVec2> = path.iter().map(|p| IVec2::new(p.x, p.y)).collect();
            commands.entity(player_entity).insert(MoveAlongPath {
                path: path_iv2,
                current_index: 1, // start at 1 since 0 is the current position
                timer: Timer::from_seconds(0.3, TimerMode::Repeating),
            });
        }
    }
    else if input.just_pressed(MouseButton::Right) {

        let mut p0 = param_set.p0();
        let (entity, transform, position) = p0.iter_mut().next().unwrap();

        println!("Reached");
        
        let path_ops = find_path(*position, game_state.0, quad_tree, camera_query, windows, PATH_DRAW_MARGIN);
        if path_ops.is_none() {
            return;
        }
        let path = path_ops.unwrap();
        if path.is_empty() {
            return;
        }
        let path_len = path.len();
        if path_len > WALKING_LIMIT {
            println!("Path too long");
            return;
        }

        println!("path len: {}", path_len);

        if path_len > 1 {
            for i in 1..path_len {
                let next_tile = path[i]; // index 0 is current tile
                // convert to world position:
                let target_x = next_tile.x as f32;
                let target_y = next_tile.y as f32;

                commands.spawn((
                    Sprite {
                        image: asset_server.load("dot.png"),
                        //image: Circle::new(10.0).into(),
                        custom_size: Some(Vec2::new(10.0, 10.0)),
                        ..default()
                    },
                    Transform::from_xyz(target_x, target_y, 0.0),
                    //.insert(FadeOutTimer(Timer::from_seconds(2.0, false)));
                    
                )).insert(FadeOutTimer(Timer::from_seconds(1.0, TimerMode::Once)));       
            }
        }
    }
}

fn find_path(
    position: Position,
    game_state: GameState,
    //cache: Res<CachedColliders>,
    quad_tree: Res<QuadTree>,
    camera_query: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    windows: Query<&Window>,
    margin: i32
) -> Option<Vec<Position>> {

    match game_state {
        
        GameState::Exploring => {
            
            let (camera, camera_transform) = camera_query.single().expect("Failed to get camera");
            let window = windows.single().expect("Failed to get window");
            
            if let Some(screen_pos) = window.cursor_position() {

                println!("Current position: ({}, {})", position.x, position.y);

                let current_position = Position { x: position.x, y: position.y };

                let _target_position = match camera.viewport_to_world_2d(camera_transform, screen_pos) {
                    Ok(target_position) => target_position,
                    Err(_) => return None,
                };

                println!("Target position: ({}, {})", _target_position.x, _target_position.y);

                let target_position: Position = Position {
                    x: _target_position.x as i32,
                    y: _target_position.y as i32,
                };

                let path = pathfinding(quad_tree, current_position, target_position, margin);
                if path.is_empty() {
                    println!("No path found, it is empty");
                    return None;
                }

                return Some(path); 
            }
            else {
                println!("No cursor position");
                return None;
            }
        }
        GameState::Interacting => {
            return None;
        }
        GameState::Battle => {
            return None;
        }
        _ => {
            return None;
        }
    }
}

fn enter_battle(
    mut game_state: ResMut<Game_State>,
    input: Res<ButtonInput<KeyCode>>, 
) {
    if input.just_pressed(KeyCode::KeyB) {
        match game_state.0 {
            GameState::Exploring => game_state.0 = GameState::Battle,
            GameState::Battle => game_state.0 = GameState::Exploring,
            GameState::Interacting => {}, 
        };
    }
}
/* 
fn draw_distance_system(
    mut param_set: ParamSet<(
        Query<(Entity, &mut Transform, &mut Position), With<Player>>,
    )>,
    game_state: Res<Game_State>,
    camera_query: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    cache: Res<CachedColliders>,
    windows: Query<&Window>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    time: Res<Time>,
    global_variables: Res<Global_Variables>
) {
    match game_state.0 {
        GameState::Exploring => {

        }
        GameState::Interacting => {

        }
        GameState::Battle => {
            let mut p0 = param_set.p0();
            let (entity, transform, position) = p0.iter_mut().next().unwrap();
        
            println!("Reached");
            
            //let path = find_path(*position, game_state.0, cache, camera_query, windows, PATH_DRAW_MARGIN);
            //let path_len = path.len();
            let path_ops = find_path(*position, game_state.0, cache, camera_query, windows, PATH_DRAW_MARGIN);
            if path_ops.is_none() {
                return;
            }
            let path = path_ops.unwrap();
            if path.is_empty() {
                return;
            }
            let path_len = path.len();
            if path_len > WALKING_LIMIT {
                println!("Path too long");
                return;
            }
        
            println!("path len: {}", path_len);
        
            if path_len > 1 {
                for i in 1..path_len {
                    let next_tile = path[i]; // index 0 is current tile
                    // convert to world position:
                    let target_x = next_tile.x as f32;
                    let target_y = next_tile.y as f32;
        
                    commands.spawn((
                        Sprite {
                            image: asset_server.load("dot.png"),
                            //image: Circle::new(10.0).into(),
                            custom_size: Some(Vec2::new(10.0, 10.0)),
                            ..default()
                        },
                        Transform::from_xyz(target_x, target_y, 0.0),
                        //.insert(FadeOutTimer(Timer::from_seconds(2.0, false)));
                        
                    )).insert(FadeOutTimer(Timer::from_seconds(3.0, TimerMode::Once)));          
                }
            }
        }
    }   
} */

fn load_dialogue() -> HashMap<String, DialogueLine> {
    let file = File::open("dialogues/example.json").unwrap();
    let reader = BufReader::new(file);
    let dialogue_lines: Vec<DialogueLine> = serde_json::from_reader(reader).unwrap();
    dialogue_lines
        .into_iter()
        .map(|line| (line.id.clone(), line))
        .collect()
}

fn display_dialogue(
    mut commands: &mut Commands,
    mut state: &mut ResMut<Dialogue_State>,
    data: &Res<Dialogue_Data>,
    mut box_query: Query<(Entity, &Children), With<DialogueBox>>,
    text_query: Query<Entity, With<DialogueText>>,
    button_query: Query<Entity, With<ChoiceButton>>,
    mut index: &mut ResMut<Selected_Choice_Index>,
    mut selected: &mut ResMut<Selected_Choice>
) {
    if let Some(current_id) = &state.0.current_id {
        if let Some(dialogue) = data.0.get(current_id) {
            println!("Dialogue found: {}", dialogue.text);
            if let Ok((box_entity, children)) = box_query.single_mut() {

                // Remove old choice buttons
                for child in children.iter() {
                    if button_query.get(child).is_ok() {
                        commands.entity(child).despawn();
                    }
                }

                // Update dialogue text
                for child in children.iter() {
                    if text_query.get(child).is_ok() {
                        commands.entity(child).insert(
                        (
                            Text::new(format!("{}: {}", dialogue.speaker, dialogue.text)),
                            TextFont {
                                font_size: 20.0,
                                ..Default::default()
                            },
                            TextColor(Color::srgb(0.9, 0.9, 0.9)), // Softer white for comfort
                            Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
                            GlobalTransform::default(),
                        ));
                    }
                }

                // Add new choice buttons
                let choices = &dialogue.choices;

                match choices {
                    Some(choices_v) => {
                        index.0 %= choices_v.len() as u8;
                        for (i, choice) in choices_v.iter().enumerate() {
                            let is_selected = index.0 == i as u8;
                            if is_selected {
                                selected.0 = choice.clone();
                            }
                            commands.entity(box_entity).with_children(|parent| {
                                parent
                                    .spawn((
                                        Button {
                                            ..default()
                                        },
                                        Node {
                                            width: Val::Px(240.0), // Wider for cleaner text
                                            height: Val::Px(45.0),
                                            margin: UiRect::vertical(Val::Px(4.0)),
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            ..default()
                                        },
                                        BackgroundColor(if is_selected {
                                            Color::srgb(0.25, 0.45, 0.25) // Highlighted
                                        } else {
                                            Color::srgb(0.15, 0.25, 0.15) // Normal
                                        }),
                                        ChoiceButton {
                                            next_id: choice.next.as_ref().unwrap().clone(),
                                        },
                                    ))
                                    .with_children(|btn| {
                                        btn.spawn((
                                            Text::new(&choice.text),
                                            TextFont {
                                                font_size: 17.0, // Slightly larger
                                                ..Default::default()
                                            },
                                            TextColor(if is_selected {
                                                Color::WHITE
                                            } else {
                                                Color::srgb(0.7, 0.7, 0.7) // Softer gray
                                            }),
                                            Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
                                            GlobalTransform::default(),
                                        ));
                                    });
                                });
                            }
                        }

                    None => {
                        println!("No choices found for ID: {}", current_id);
                    }
                }
            } else {
                println!("No dialogue box found, it has not spawned yet");
            }

        } else {
            println!("No dialogue found for ID: {}", current_id);
        }
    } else {
        println!("No current ID, time to despawn");

        for (box_entity, children) in box_query.iter_mut() {
            for child in children.iter() {
                if button_query.get(child).is_ok() {
                    commands.entity(child).despawn();
                }
            }
            commands.entity(box_entity).despawn();
        }

        //spawn_dialogue_box(&mut commands);        
    }
}

fn handle_choice_event(
    event: u32,
    mut next_id_map: ResMut<Next_Id>,
    mut conditionals: ResMut<Conditionals>
) {
    match event {
        0 => println!("Choice 1 selected"),
        1 => println!("Choice 2 selected"),
        _ => println!("Invalid choice"),
    }
}

fn handle_next_id(
    id: Option<String>,
    next_id_map: &ResMut<Next_Id>
) -> Option<String> {
    let return_id = match id {
        None => None,
        Some(id) => {
            let next_id = match next_id_map.0.get(&id) {
                None => Some(id.clone()),
                Some(next_id) => Some(next_id.clone()),
            };
            next_id
        },
    };
    return_id
}


// struct Ray {
//     origin: Vec2,
//     direction: Vec2, // Should be normalized
// }

// fn ray_intersects_segment(ray_origin: Vec2, ray_dir: Vec2, p1: Vec2, p2: Vec2) -> Option<Vec2> {
//     let v1 = ray_origin - p1;
//     let v2 = p2 - p1;
//     let v3 = Vec2::new(-ray_dir.y, ray_dir.x);

//     let dot = v2.dot(v3);
//     if dot.abs() < f32::EPSILON {
//         return None; // Parallel
//     }

//     let t1 = v2.perp_dot(v1) / dot;
//     let t2 = v1.dot(v3) / dot;

//     if t1 >= 0.0 && t2 >= 0.0 && t2 <= 1.0 {
//         Some(ray_origin + ray_dir * t1)
//     } else {
//         None
//     }
// }

// fn raycast_against_rect(ray: &Ray, rect: Rect) -> Option<Vec2> {
//     let Rect { min, max } = rect;

//     let corners = [
//         Vec2::new(min.x, min.y),
//         Vec2::new(max.x, min.y),
//         Vec2::new(max.x, max.y),
//         Vec2::new(min.x, max.y),
//     ];

//     let edges = [
//         (corners[0], corners[1]),
//         (corners[1], corners[2]),
//         (corners[2], corners[3]),
//         (corners[3], corners[0]),
//     ];

//     edges.iter()
//         .filter_map(|(start, end)| ray_intersects_segment(ray.origin, ray.direction, *start, *end))
//         .min_by(|a, b| {
//             let da = a.distance_squared(ray.origin);
//             let db = b.distance_squared(ray.origin);
//             da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
//         })
// }

// #[derive(Component)]
// struct LightSegment<'a> {
//     emissor: &'a Entity,
// }

// fn spawn_light_ray_segments(
//     commands: &mut Commands,
//     origin: &Vec2,
//     hit: Vec2,
//     perlin: &Perlin,
//     time: f32,
// ) {
//     let direction = hit - origin;
//     let length = direction.length();
//     let dir_normalized = direction.normalize();
//     let segment_count = (length / SEGMENT_SIZE).ceil() as usize;

//     for i in 0..segment_count {
//         let t = i as f32 / segment_count as f32;
//         let base_pos = origin + dir_normalized * t * length;

//         // Apply Perlin noise offset (vertical to ray direction)
//         let perp = Vec2::new(-dir_normalized.y, dir_normalized.x);
//         let noise_val = perlin.get([
//             base_pos.x as f64 * NOISE_SCALE,
//             base_pos.y as f64 * NOISE_SCALE,
//             time as f64 * 0.5,
//         ]) as f32;

//         let offset = perp * noise_val * 3.0;
//         let final_pos = base_pos + offset;

//         commands.spawn((
//             Sprite {
//                 color: Color::srgba(1.0, 1.0, 0.8, LIGHT_ALPHA),
//                 custom_size: Some(Vec2::new(SEGMENT_SIZE, LIGHT_WIDTH)),
//                 ..default()
//             },
//             Transform {
//                 translation: final_pos.extend(5.0),
//                 rotation: Quat::from_rotation_z(dir_normalized.y.atan2(dir_normalized.x)),
//                 ..default()
//             },
//             GlobalTransform::default(), 
//             Visibility::Inherited,
//             InheritedVisibility::default(),
//             ViewVisibility::default(), 
//             LightSegment,
//         ));
//     }
// }

// fn cleanup_light_segments(mut commands: &mut Commands, query: Query<(Entity, &LightSegment)>, emissor: &Entity) {
//     for (entity, light_segment) in &query {
//         if light_segment.emissor != emissor { continue; }
//         commands.entity(entity).despawn();
//     }
// }

// fn emit_light(
//     mut commands: &mut Commands,
//     time: &Res<Time>,
//     origin: Vec2,
//     camera_position: Vec2,
//     //player_query: Query<&Transform, With<Player>>, // it doesn't need to be the player
//     //cache: &Res<CachedColliders>,
//     quadTree: &Res<QuadTree>,
// ) {
//     if origin.distance_squared(camera_position) <= MAX_DISTANCE_RENDER {
    
//         //let player_pos = player_query.single().unwrap().translation.truncate();
//         for i in 0..72 {

//             let angle_deg = i as f32 * 5.0;
//             let angle_rad = angle_deg.to_radians();

//             let ray_dir = Vec2::new(angle_rad.cos(), angle_rad.sin()); // unit circle direction

//             let ray = Ray {
//                 origin,
//                 direction: ray_dir.normalize(),
//             };

//             let perlin = Perlin::new(42);

//             let mut nearby = Vec::new();
//             let ray_bounds = Rect::from_center_size(ray.origin, Vec2::splat(300.0)); // light radius
//             //cached_colliders.quadtree.query(ray_bounds, &mut nearby);
//             quadTree.0.query(ray_bounds, &mut nearby);

//             for collider in nearby  {
                
//                 // let rect = Rect::from_center_size(
//                 //     collider.translation.truncate(),
//                 //     Vec2::splat(32.0),
//                 // );

//                 if let Some(hit) = raycast_against_rect(&ray, collider.bounds) {
//                     spawn_light_ray_segments(
//                         &mut commands,
//                         &ray.origin,
//                         hit,
//                         &perlin,
//                         time.elapsed_secs(),
//                     );
//                     break; // stop after first collision
//                 }
//             }
//         }
//     }
// }

impl QuadtreeNode {

    pub fn new(bounds: Rect, level: usize) -> Self {
        Self {
            bounds,
            level,
            objects: Vec::new(),
            children: None,
        }
    }
    
    fn subdivide(&mut self) {
        let center = self.bounds.center();
        let half_size = self.bounds.size() / 2.0;
    
        let [min, max] = [self.bounds.min, self.bounds.max];
        let mid = center;
    
        self.children = Some([
            Box::new(QuadtreeNode::new(Rect::from_corners(min, mid), self.level + 1)), // bottom-left
            Box::new(QuadtreeNode::new(Rect::from_corners(Vec2::new(mid.x, min.y), Vec2::new(max.x, mid.y)), self.level + 1)), // bottom-right
            Box::new(QuadtreeNode::new(Rect::from_corners(Vec2::new(min.x, mid.y), Vec2::new(mid.x, max.y)), self.level + 1)), // top-left
            Box::new(QuadtreeNode::new(Rect::from_corners(mid, max), self.level + 1)), // top-right
        ]);
    }
    
    pub fn insert(&mut self, collider: Collider) {
        if !aabb_collision(self.bounds, collider.bounds) {
            return;
        }
    
        if self.children.is_some() {
            if let Some(children) = &mut self.children {
                for child in children.iter_mut() {
                    if child.bounds.contains(collider.bounds.center()) {
                        child.insert(collider);
                        return;
                    }
                }
            }
        }
    
        self.objects.push(collider);
    
        if self.objects.len() > MAX_OBJECTS && self.level < MAX_LEVELS {
            if self.children.is_none() {
                self.subdivide();
            }
    
            if let Some(children) = &mut self.children {
                let mut reinsert = Vec::new();
                std::mem::swap(&mut self.objects, &mut reinsert);
                for obj in reinsert {
                    self.insert(obj);
                }
            }
        }
    }
    
    pub fn query<'a>(&'a self, area: Rect, found: &mut Vec<&'a Collider>) {
        if !aabb_collision(self.bounds, area) {
            return;
        }
    
        for collider in &self.objects {
            if aabb_collision(collider.bounds, area) {
                found.push(collider);
            }
        }
    
        if let Some(children) = &self.children {
            for child in children {
                child.query(area, found);
            }
        }
    }
}

impl Default for QuadtreeNode {
    fn default() -> Self {
        Self {
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            level: 0,
            objects: Vec::new(),
            children: None,
        }
    }
}