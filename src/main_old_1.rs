use bevy::core_pipeline::core_2d::graph::input;
use bevy::ecs::entity;
use bevy::math::ops::powf;
use bevy::prelude::*;
use bevy::ui::prelude::*;
use bevy::ui::{PositionType};
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
//use svg::*;
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;
use std::hash::Hasher;
use std::cmp::Ordering;
//use approx::abs_diff_eq::AbsDiffEq;
use approx::AbsDiffEq;
use serde::Deserialize;
use serde_json::*;
use std::fs::File;
use std::io::BufReader;

const WINDOW_WIDTH: f32 = 640.0;
const WINDOW_HEIGHT: f32 = 480.0;
const PLAYER_SPEED: f32 = 200.0;

const GRID_WIDTH: u32 = 15000;
const GRID_HEIGHT: u32 = 15000;

const PATH_MARGIN: i32 = 5;
const PATH_DRAW_MARGIN: i32 = 10;
const PATH_MOVEMENT_SPEED: u32 = 10;

const WALKING_LIMIT: usize = 40;

//static GAME_STATE: Lazy<Arc<RwLock<GameState>>> = Lazy::new(|| {
//    Arc::new(RwLock::new(GameState::Exploring))
//});

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
}

impl Default for DialogueState {
    fn default() -> Self {
        DialogueState {
            current_id: None,
            active: false,
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
    name: String,
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
        mut commands: Commands,
        mut state: ResMut<Dialogue_State>,
        data: Res<Dialogue_Data>,
        asset_server: Res<AssetServer>,
        mut box_query: Query<(Entity, &Children), With<DialogueBox>>,
        text_query: Query<Entity, With<DialogueText>>,
        button_query: Query<Entity, With<ChoiceButton>>,
    ) {
        // code to handle interaction goes here
        let previous_state = game_state.clone();
        game_state = GameState::Interacting;
        println!("Interacting, game state: {:?}", game_state);
        state.0.current_id = Some(self.dialogue_id.clone());
        state.0.active = true;
        //display_dialogue(commands, &state, data, asset_server, box_query, text_query, button_query);
        //game_state = previous_state;
        //state.0.current_id = None;
        //state.0.active = false;
        //println!("Interaction finished, game state: {:?}", game_state);
        //println!("Interacted with {}, on transform: {:?}", self.name, transform);
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

/* // New resource to keep track of the collision grid
#[derive(Resource)]
struct CollisionGrid(Vec<Vec<bool>>); */

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
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.1, 0.1, 0.1)))
        .insert_resource(CachedInteractables(Vec::new()))
        .insert_resource(CachedColliders(Vec::new()))
        .insert_resource(Game_State(GameState::Exploring))
        .insert_resource(Global_Variables(GlobalVariables::default()))
        .insert_resource(Dialogue_State(DialogueState::default()))
        .insert_resource(Selected_Choice(Choice::default()))
        .insert_resource(Selected_Choice_Index(0))
        .insert_resource(DialogueTrigger(false))
        .insert_resource(DialogueJustSpawned(false))
        .add_systems(Startup, setup)
        .add_systems(Update, player_movement)
        .add_systems(Update, update_cache)
        .add_systems(Update, mouse_click)
        .add_systems(Update, follow_path_system)
        .add_systems(Update, enter_battle)
        .add_systems(Update, spawn_dialogue_box.in_set(DialogueSet::Spawn))
        .add_systems(Update, interact.in_set(DialogueSet::Interact).after(DialogueSet::Spawn))
        .add_systems(Update, create_first_dialogue)
        .add_systems(Update, gui_selection)
        .run();
}

fn create_first_dialogue(
    mut commands: Commands,
    state: ResMut<Dialogue_State>,
    data: Res<Dialogue_Data>,
    asset_server: Res<AssetServer>,
    box_query: Query<(Entity, &Children), With<DialogueBox>>,
    text_query: Query<Entity, With<DialogueText>>,
    button_query: Query<Entity, With<ChoiceButton>>,
    mut just_spawned: ResMut<DialogueJustSpawned>,
    mut index: ResMut<Selected_Choice_Index>,
    mut selected: ResMut<Selected_Choice>
) {
    if just_spawned.0 {
        display_dialogue(commands, &state, data, asset_server, box_query, text_query, button_query, index, selected);
        just_spawned.0 = false;
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
pub struct CachedColliders(Vec<(Transform, Collider)>);

#[derive(Resource, Default)]
pub struct Dialogue_State(DialogueState);

#[derive(Resource, Default)]
pub struct Dialogue_Data(pub HashMap<String, DialogueLine>);

#[derive(Resource, Default)]
pub struct Selected_Choice(Choice);

#[derive(Resource, Default)]
pub struct Selected_Choice_Index(u8);

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

#[derive(Resource)]
struct DialogueTrigger(bool);

#[derive(Resource, Default)]
struct DialogueJustSpawned(bool);

fn spawn_dialogue_box(
    mut commands: Commands,
    mut trigger: ResMut<DialogueTrigger>,
    mut just_spawned: ResMut<DialogueJustSpawned>,
) {
    if trigger.0 {
        println!("Function called");
        commands
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(200.0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Start,
                    align_items: AlignItems::Start,
                    padding: UiRect::all(Val::Px(10.0)),
                    ..default()
            },
                BackgroundColor(Color::srgb(0.15, 0.15, 0.15)),
                DialogueBox,
            ))
            .with_children(|parent| {
                parent.spawn((
                    TextFont {
                            //font: asset_server.load("fonts/FiraSans-Bold.ttf"),
                            font_size: 16.0,
                            ..Default::default()
                        },
                    TextColor(Color::WHITE),
                    Text::new(""),
                    DialogueText,
                ));
            });
        trigger.0 = false;
        just_spawned.0 = true;
    }
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {

    let dialogue_data = load_dialogue();
    commands.insert_resource(Dialogue_Data(dialogue_data));
    
    commands
    .spawn(Camera2d::default())
    .insert(MainCamera);
    //.insert(Player)
    //.insert(Position { x: 0, y: 0 });

    //commands.insert_resource(CachedInteractables(Vec::new()));
    //commands.insert_resource(CachedColliders(Vec::new()));
    

    //let texture_handle = asset_server.load("character.png");

    // Create a new collision grid
    /* let mut collision_grid = vec![vec![false; GRID_WIDTH]; GRID_HEIGHT];

    // Add some walls to the collision grid
    collision_grid[4][5] = true;
    collision_grid[4][6] = true;

    // Insert the collision grid as a resource
    commands.insert_resource(CollisionGrid(collision_grid)); */

    // Spawn the player
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
            Collider { name: "Wall".to_string() },
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
    /* mut query: Query<(&mut Transform, &mut Position), With<Player>>,
    collider_query: Query<&Transform, With<Collider>>, */
    mut param_set: ParamSet<(
        Query<(&mut Transform, &mut Position), With<Player>>,
        //Query<&Transform, With<Collider>>,
    )>,
    game_state: Res<Game_State>,
    cache: Res<CachedColliders>,
    input: Res<ButtonInput<KeyCode>>, 
    time: Res<Time>,
    mut index: ResMut<Selected_Choice_Index>,
    //collision_grid: Res<CollisionGrid>,
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

    if direction.length() > 0.0 {
        match game_state.0 {
            GameState::Exploring => {
                if direction.x != 0.0 && direction.y != 0.0 {
                    // Diagonal movement
                    let diagonal_speed = movement_speed / (2.0_f32.sqrt());
                     // First, collect all collider data safely
                    //let p1 = param_set.p1();
                    //let colliders: Vec<_> = p1.iter().cloned().collect();
                    //let colliders: Vec<_> = p1.iter().collect();
        
                    let mut p0 = param_set.p0();
        
                    for (mut transform, mut position) in p0.iter_mut() {
                        let new_x = transform.translation.x + direction.x * diagonal_speed;
                        let new_y = transform.translation.y + direction.y * diagonal_speed;
        
                        // Check if the new position is within the collision grid
                        /* let grid_x = (new_x / 32.0) as usize;
                        let grid_y = (new_y / 32.0) as usize; */
                        transform.rotation = Quat::from_rotation_z(
                            rotate_to_direction(transform.translation.x, transform.translation.y, new_x, new_y),
                        );
        
                        if ((new_x.abs() as u32) < GRID_WIDTH) && ((new_y.abs() as u32) < GRID_HEIGHT) {
                            /* transform.translation.x = new_x;
                            transform.translation.y = new_y;
                            position.x = new_x as i32;
                            position.y = new_y as i32; */
        
                             // Collision detection
                            let player_rect = Rect::from_center_size(Vec2::new(new_x, new_y), Vec2::new(32.0, 32.0));
        
                            // Check collision using AABB
                            let collision = cache.0.iter().any(|wall_transform| {
                                let wall_rect = Rect::from_center_size(
                                    Vec2::new(wall_transform.0.translation.x, wall_transform.0.translation.y),
                                    Vec2::new(32.0, 32.0),
                                );
                                aabb_collision(player_rect, wall_rect)
                            });
        
                            if !collision {
                                transform.translation.x = new_x;
                                transform.translation.y = new_y;
                                position.x = new_x as i32;
                                position.y = new_y as i32;
                            }
                        }
                    }
                } else {
        
                    //let p1 = param_set.p1();
                    //let colliders: Vec<_> = p1.iter().cloned().collect();
                    // Horizontal or vertical movement
                    for (mut transform, mut position) in param_set.p0().iter_mut() {
                        let new_x = transform.translation.x + direction.x * movement_speed;
                        let new_y = transform.translation.y + direction.y * movement_speed;
        
                        // Check if the new position is within the collision grid
                        /* let grid_x = (new_x / 32.0) as usize;
                        let grid_y = (new_y / 32.0) as usize; */
                        transform.rotation = Quat::from_rotation_z(
                            rotate_to_direction(transform.translation.x, transform.translation.y, new_x, new_y),
                        );
        
                        if ((new_x.abs() as u32) < GRID_WIDTH) && ((new_y.abs() as u32) < GRID_HEIGHT) {
                            /* transform.translation.x = new_x;
                            transform.translation.y = new_y;
                            position.x = new_x as i32;
                            position.y = new_y as i32; */
        
                            let player_rect = Rect::from_center_size(Vec2::new(new_x, new_y), Vec2::new(32.0, 32.0));
        
                            // Check collision using AABB
                            let collision = cache.0.iter().any(|wall_transform| {
                                let wall_rect = Rect::from_center_size(
                                    Vec2::new(wall_transform.0.translation.x, wall_transform.0.translation.y),
                                    Vec2::new(32.0, 32.0),
                                );
                                aabb_collision(player_rect, wall_rect)
                            });
        
                            if !collision {
                                transform.translation.x = new_x;
                                transform.translation.y = new_y;
                                position.x = new_x as i32;
                                position.y = new_y as i32;
                            }
                        }
                    }
                }
            }
            GameState::Battle => {
                //direction = direction.normalize();
            }
            GameState::Interacting => {
                //direction = direction.normalize();
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
    mut selected: ResMut<Selected_Choice>
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
                    display_dialogue(commands, &state, data, asset_server, box_query, text_query, button_query, index, selected);
            }
        }
    }

}

fn interact<'a>(
    mut param_set: ParamSet<(
        Query<(&Transform, &Position), With<Player>>,
        //Query<(&Transform, &Interactable), With<Interactable>>,
        //Query<&Interactable, With<Interactable>>
    )>,
    mut game_state: ResMut<Game_State>,
    cache: Res<CachedInteractables>,
    input: Res<ButtonInput<KeyCode>>, 
    mut dialogue_state: ResMut<Dialogue_State>,
    dialogue_data: Res<Dialogue_Data>,
    mut selected_choice: ResMut<Selected_Choice>,
    mut dialogue_trigger: ResMut<DialogueTrigger>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut box_query: Query<(Entity, &Children), With<DialogueBox>>,
    text_query: Query<Entity, With<DialogueText>>,
    button_query: Query<Entity, With<ChoiceButton>>,
    mut index: ResMut<Selected_Choice_Index>,
) {
    if (input.just_pressed(KeyCode::KeyX)) {
        
        match game_state.0 {
            GameState::Exploring => {
                //let p1 = param_set.p1();
        
                /* let interactables: Vec<(Transform, Interactable)> = param_set
                    .p1()
                    .iter()
                    .map(|(t, i)| (t, i)) 
                    .collect(); */
            
                //let interactables: Vec<(Rc<&Transform>, Rc<&Interactable>)> = p1.iter().map(|(t, i)| (Rc::new(t), Rc::new(i))).collect();
                
                let p0 = param_set.p0();
            
                //let interactables: Vec<(_, _)> = p1.iter().map(|(t, i)| (t, i)).collect();
            
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
                        dialogue_trigger.0 = true;
                        println!("Triggered");
                        interactable.interact(interactable_transform, game_state.0, commands, dialogue_state, dialogue_data, asset_server, box_query, text_query, button_query);
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
    else if (input.just_pressed(KeyCode::Space)) {
        match game_state.0 {
            GameState::Exploring => {

            }
            GameState::Battle => {
                
            }
            GameState::Interacting => {
                println!("Space pressed when interacting");
                if let Some(current_id) = &dialogue_state.0.current_id {
                    if let Some(line) = dialogue_data.0.get(current_id) {
                        /* if let Some(next_id) = &line.next {
                            let next_id_parts: Vec<&str> = next_id.split_whitespace().collect();
                            if next_id_parts[0] == "#(CHOICE)" {
                                let choices: &Vec<Choice> = line.choices.as_ref().unwrap();
                                for choice in choices {
                                    
                                }
                            }
                            else {
                                dialogue_state.current_id = Some(next_id.clone());
                            }
                        }
                        else {
                            dialogue_state.active = false;
                        } */
                        let current_choices = &line.choices;
                        if current_choices.is_some() {
                            if selected_choice.0.next.is_none() {
                                return;
                            }
                            dialogue_state.0.current_id = selected_choice.0.next.clone();
                            handle_choice_event(selected_choice.0.event);
                        }
                        else {
                            dialogue_state.0.current_id = line.next.clone();
                            if dialogue_state.0.current_id.is_none() {
                                dialogue_state.0.active = false;
                                game_state.0 = GameState::Exploring;
                            }
                        }
                        display_dialogue(commands, &dialogue_state, dialogue_data, asset_server, box_query, text_query, button_query, index, selected_choice);
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

fn update_collider_cache(
    mut cache: ResMut<CachedColliders>,
    query: Query<(&Transform, &Collider), With<Collider>>,
) {
    cache.0 = query
        .iter()
        .map(|(t, i)| (t.clone(), i.clone()))
        .collect();
}

fn update_cache(
    cache_interactables: ResMut<CachedInteractables>,
    cache_colliders: ResMut<CachedColliders>,
    interactable_query: Query<(&Transform, &Interactable), With<Interactable>>,
    collider_query: Query<(&Transform, &Collider), With<Collider>>,
    input: Res<ButtonInput<KeyCode>>, 
) {
    if(input.just_pressed(KeyCode::KeyP)) {
        update_interactable_cache(cache_interactables, interactable_query);
        update_collider_cache(cache_colliders, collider_query);
    }
}

/* #[derive(Clone, Copy, Debug)]
struct Coord {
    x: f32,
    y: f32,
}

impl PartialEq for Coord {
    fn eq(&self, other: &Self) -> bool {
        self.x.abs_diff_eq(&other.x, 1e-6) && self.y.abs_diff_eq(&other.y, 1e-6)
    }
}

impl Eq for Coord {}

impl Hash for Coord {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.x.to_bits().hash(state);
        self.y.to_bits().hash(state);
    }
}


#[derive(Copy, Clone)]
struct Node_P {
    translation: Coord,
    cost: f32,         // f = g + h
    priority: f32,     // used for ordering
}

impl PartialEq for Node_P {
    fn eq(&self, other: &Self) -> bool {
        self.translation.x.abs_diff_eq(&other.translation.x, 1e-6) &&
        self.translation.y.abs_diff_eq(&other.translation.y, 1e-6) &&
        self.cost == other.cost &&
        self.priority == other.priority
    }
}

impl Eq for Node_P {}

impl Ord for Node_P {
    fn cmp(&self, other: &Self) -> Ordering {
        other.priority.cmp(&self.priority) // reverse for min-heap
            .then_with(|| self.cost.cmp(&other.cost))
    }
}
impl PartialOrd for Node_P {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn distance(a: Coord, b: Coord) -> f32 {
    (a.x - b.x).abs() + (a.y - b.y).abs()
}

fn is_walkable(pos: Coord, cache: &CachedColliders) -> bool {
    let pos_center = Vec2::new(pos.x as f32 * 32.0 + 16.0, pos.y as f32 * 32.0 + 16.0);
    let player_rect = Rect::from_center_size(pos_center, Vec2::new(32.0, 32.0));

    !cache.0.iter().any(|(wall_transform, _)| {
        let wall_rect = Rect::from_center_size(
            wall_transform.translation.truncate(),
            Vec2::new(32.0, 32.0),
        );
        aabb_collision(player_rect, wall_rect)
    })
}

fn pathfinding(
    cache: Res<CachedColliders>,
    start: Coord,
    destination: Coord,
) -> Option<Vec<Coord>> {
    let mut circle = Circle::new(10.0);
    //circle.set("r", "10");
    //circle.set("fill", "red");
    let mut open_set = BinaryHeap::new();
    let coord = Coord { x: start.x, y: start.y };
    open_set.push(Node_P {
        translation: coord,
        cost: 0.0,
        priority: distance(start, destination),
    });

    let mut came_from: HashMap<Coord, Coord> = HashMap::new();
    let mut g_score: HashMap<Coord, f32> = HashMap::new();
    g_score.insert(coord, 0.0);

    let mut visited: HashSet<Coord> = HashSet::new();

    while let Some(current) = open_set.pop() {
        if (current.translation.x - destination.x).abs() <= 0.2 && (current.translation.y - destination.y).abs() <= 0.2 {
            // reconstruct path
            let mut path = vec![destination];
            let mut curr = destination;
            while let Some(&prev) = came_from.get(&curr) {
                path.push(prev);
                curr = prev;
            }
            path.reverse();
            return Some(path);
        }

        if visited.contains(&current.translation) {
            continue;
        }
        visited.insert(current.translation);

        let neighbors = [
            Coord { x: current.translation.x + 1.0, y: current.translation.y },
            Coord { x: current.translation.x - 1.0, y: current.translation.y },
            Coord { x: current.translation.x, y: current.translation.y + 1.0 },
            Coord { x: current.translation.x, y: current.translation.y - 1.0 },
        ];

        for neighbor in neighbors {
            if neighbor.x < 0.0 || neighbor.y < 0.0 || 
                neighbor.x >= GRID_WIDTH as f32 || neighbor.y >= GRID_HEIGHT as f32 {
                continue;
            }

            if !is_walkable(neighbor, &cache) {
                continue;
            }

            let tentative_g: f32 = g_score.get(&current.translation).unwrap_or(&f32::MAX) + 1.0;
            if tentative_g < *g_score.get(&neighbor).unwrap_or(&f32::MAX) {
                came_from.insert(neighbor, current.translation);
                g_score.insert(neighbor, tentative_g);

                open_set.push(Node_P {
                    translation: neighbor,
                    cost: tentative_g,
                    priority: tentative_g + distance(neighbor, destination),
                });
            }
        }
    }

    None // No path found
} */

#[derive(Copy, Clone, Eq, PartialEq)]
struct Node_P {
    position: Position,
    cost: i32,         // f = g + h
    priority: i32,     // used for ordering
}

/* impl Ord for Node_P {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.cost + self.priority).cmp(&(other.cost + other.priority))
    }
} */
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

fn is_walkable(pos: Position, cache: &CachedColliders) -> bool {
    if pos.x.abs() as u32 > GRID_WIDTH || pos.y.abs() as u32 > GRID_HEIGHT {
        return false;
    }
    let pos_center = Vec2::new(pos.x as f32, pos.y as f32);
    let player_rect = Rect::from_center_size(pos_center, Vec2::new(32.0, 32.0));

    !cache.0.iter().any(|(wall_transform, _)| {
        let wall_rect = Rect::from_center_size(
            wall_transform.translation.truncate(),
            Vec2::new(32.0, 32.0),
        );
        aabb_collision(player_rect, wall_rect)
    })
}

pub fn pathfinding(
    cache: Res<CachedColliders>,
    start: Position,
    goal: Position,
    margin: i32
) -> Vec<Position> {

    if !is_walkable(start, &cache) || !is_walkable(goal, &cache) {
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

    //println!("Reached before the while loop");

    while !((next_Node_P.position.x - goal.x).abs() < margin && (next_Node_P.position.y - goal.y).abs() < margin) {

        //println!("Reached the while loop 1");

        //let mut current = next_Node_P;

    //while let Some(current) = open_set.pop() {
        /* if current.position.x - goal.x < 2 && current.position.y - goal.y < 2 {
            // reconstruct path
            
        }
 */
        //println!("Reached the while loop 2");
        if visited.contains(&next_Node_P.position) {
            next_Node_P = open_set.pop().unwrap();
            continue;
        }
        //println!("Reached the while loop 3");
        if visited.len() > 1000 {
            let mut previou_Node_P_position = next_Node_P.position;
            while previou_Node_P_position != start {
                println!("Previous Node_P: {:#?}", previou_Node_P_position);
                previou_Node_P_position = came_from.get(&previou_Node_P_position).unwrap().clone();
            }
            //println!("visited too many Node_Ps, \nstart {:#?}, \ngoal {:#?}, \nend {:#?}", start, goal, next_Node_P.position);
            break;
        }
        //println!("Reached the while loop 4");
        visited.insert(next_Node_P.position);
        //println!("Reached the while loop 5");

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
        //println!("Reached the while loop 6");
        //println!("neighbors: {:#?}", neighbors);

        for neighbor in neighbors {

            if !is_walkable(neighbor, &cache) {
                println!("Skipped neighbor collider: ({}, {})", neighbor.x, neighbor.y);
                continue;
            }

            let movement_cost = if neighbor.x == next_Node_P.position.x || neighbor.y == next_Node_P.position.y {
                10
            } else {
                14
            };

            let tentative_g = g_score.get(&next_Node_P.position).unwrap_or(&i32::MAX) + movement_cost;
            //println!("tentative_g: {} g_score: {}", tentative_g, g_score.get(&neighbor).unwrap_or(&i32::MAX));
            if tentative_g < *g_score.get(&neighbor).unwrap_or(&i32::MAX) {
                //println!("Added neighbor: ({}, {})", neighbor.x, neighbor.y);
                came_from.insert(neighbor, next_Node_P.position);
                g_score.insert(neighbor, tentative_g);

                open_set.push(Node_P {
                    position: neighbor,
                    cost: tentative_g,
                    priority: tentative_g + distance(neighbor, goal),
                });
            }
            else {
                //println!("Skipped neighbor: ({}, {})", neighbor.x, neighbor.y);
            }
        }
        let old_Node_P = next_Node_P;
        //println!("next_Node_P before: ({}, {})", next_Node_P.position.x, next_Node_P.position.y);
        next_Node_P = open_set.pop().unwrap();
        //println!("next_Node_P after: ({}, {})", next_Node_P.position.x, next_Node_P.position.y);
        if next_Node_P == old_Node_P {
            for Node_P in open_set.iter() {
                //println!("Open set: ({}, {})", Node_P.position.x, Node_P.position.y);
                //println!("Cost: {}", Node_P.cost);
                //println!("Priority: {}\n", Node_P.priority);
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

    //println!("Finished, \nstart {:#?}, \ngoal {:#?}, \nend {:#?}", start, goal, next_Node_P.position);
    //println!("Path length: {}", path.len());
    return path;

    /* println!("Pathfinding failed");

    vec![]  */// No path found
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
    mut query: Query<(&mut Transform, &mut Position, &mut MoveAlongPath, Entity)>,
    time: Res<Time>,
    mut global_variables: ResMut<Global_Variables>,
) {
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
            } else {
                commands.entity(entity).remove::<MoveAlongPath>();
                //commands.entity(entity).despawn();
                // Finished path
                // You can optionally remove the component here
                // query.entity(entity).remove::<MoveAlongPath>();
            }
        }
    }
    global_variables.0.moving = false;
}

fn toggle_camera_lock(
    mut commands: Commands,
    input: Res<ButtonInput<KeyCode>>,
    mut global_variables: ResMut<Global_Variables>,
) {
    if input.just_pressed(KeyCode::KeyL) {
        
        global_variables.0.camera_locked = !global_variables.0.camera_locked;
    }
}

fn mouse_click(
    mut param_set: ParamSet<(
        Query<(Entity, &mut Transform, &mut Position), With<Player>>,
        //Query<(&Transform, &Interactable), With<Interactable>>,
        //Query<&Interactable, With<Interactable>>
    )>,
    game_state: Res<Game_State>,
    camera_query: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    cache: Res<CachedColliders>,
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
        //let (mut transform, mut position) = p0.iter_mut().next().unwrap();
        
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
            let path_iv2: Vec<IVec2> = path.iter().map(|p| IVec2::new(p.x, p.y)).collect();
            commands.entity(player_entity).insert(MoveAlongPath {
                path: path_iv2,
                current_index: 1, // start at 1 since 0 is the current position
                timer: Timer::from_seconds(0.3, TimerMode::Repeating),
            });
        }

        /* if path_len > 1 {
            let mut i = 1;
            let mut timer = Timer::from_seconds(0.5, TimerMode::Repeating); // adjust the speed here
    
            while i < path_len {
                if timer.tick(time.delta()).just_finished() {
                    let next_tile = path[i]; // index 0 is current tile
                    // convert to world position:
                    let target_x = next_tile.x as f32;
                    let target_y = next_tile.y as f32;
                    transform.rotation = Quat::from_rotation_z(rotate_to_direction(transform.translation.x, transform.translation.y, target_x, target_y));
                    transform.translation.x = target_x;
                    transform.translation.y = target_y;
                    position.x = next_tile.x;
                    position.y = next_tile.y;
                    i += 1;
                    timer.reset(); // reset the timer for the next step
                }
            }
        } */
    }
    else if input.just_pressed(MouseButton::Right) {

        let mut p0 = param_set.p0();
        let (entity, transform, position) = p0.iter_mut().next().unwrap();

        println!("Reached");
        
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
                    
                )).insert(FadeOutTimer(Timer::from_seconds(1.0, TimerMode::Once)));       
            }
        }
    }
}

fn find_path(
    position: Position,
    game_state: GameState,
    cache: Res<CachedColliders>,
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

                let path = pathfinding(cache, current_position, target_position, margin);
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
}

/* if direction.length() > 0.0 {
    if direction.x != 0.0 && direction.y != 0.0 {
        // Diagonal movement
        let diagonal_speed = movement_speed / (2.0_f32.sqrt());
        // First, detect collisions and store the results
        let collisions: Vec<bool> = param_set.p0().iter().zip(param_set.p1().iter()).map(|((transform, _), wall_transform)| {
            let player_rect = Rect::from_center_size(Vec2::new(transform.translation.x, transform.translation.y), Vec2::new(32.0, 32.0));
            let wall_rect = Rect::from_center_size(Vec2::new(wall_transform.translation.x, wall_transform.translation.y), Vec2::new(32.0, 32.0));
            aabb_collision(player_rect, wall_rect)
        }).collect();

        // Then, update the positions based on the collision results
        for (i, (mut transform, mut position)) in param_set.p0().iter_mut().enumerate() {
            let new_x = transform.translation.x + direction.x * diagonal_speed;
            let new_y = transform.translation.y + direction.y * diagonal_speed;

            // Check if the new position is within the collision grid
            let grid_x = (new_x / 32.0) as usize;
            let grid_y = (new_y / 32.0) as usize;

            if grid_x < GRID_WIDTH && grid_y < GRID_HEIGHT {
                if !collisions[i] {
                    transform.translation.x = new_x;
                    transform.translation.y = new_y;
                    position.x = new_x as i32;
                    position.y = new_y as i32;
                }
            }
        }
    } else {
        // Horizontal or vertical movement
        // First, detect collisions and store the results
        let collisions: Vec<bool> = param_set.p0().iter().zip(param_set.p1().iter()).map(|((transform, _), wall_transform)| {
            let player_rect = Rect::from_center_size(Vec2::new(transform.translation.x, transform.translation.y), Vec2::new(32.0, 32.0));
            let wall_rect = Rect::from_center_size(Vec2::new(wall_transform.translation.x, wall_transform.translation.y), Vec2::new(32.0, 32.0));
            aabb_collision(player_rect, wall_rect)
        }).collect();

        // Then, update the positions based on the collision results
        for (i, (mut transform, mut position)) in param_set.p0().iter_mut().enumerate() {
            let new_x = transform.translation.x + direction.x * movement_speed;
            let new_y = transform.translation.y + direction.y * movement_speed;

            // Check if the new position is within the collision grid
            let grid_x = (new_x / 32.0) as usize;
            let grid_y = (new_y / 32.0) as usize;

            if grid_x < GRID_WIDTH && grid_y < GRID_HEIGHT {
                if !collisions[i] {
                    transform.translation.x = new_x;
                    transform.translation.y = new_y;
                    position.x = new_x as i32;
                    position.y = new_y as i32;
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

/* fn show_dialogue(
    mut query: Query<&mut Text>,
    dialogue_data: Res<DialogueData>,
    mut state: ResMut<DialogueState>,
) {
    if let Some(current_id) = &state.current_id {
        if let Some(line) = dialogue_data.0.get(current_id) {
            for mut text in query.iter_mut() {
                text.0 = format!("{}: {}", line.speaker, line.text);
            }
        }
    }
} */

fn display_dialogue(
    mut commands: Commands,
    mut state: &ResMut<Dialogue_State>,
    data: Res<Dialogue_Data>,
    asset_server: Res<AssetServer>,
    mut box_query: Query<(Entity, &Children), With<DialogueBox>>,
    text_query: Query<Entity, With<DialogueText>>,
    button_query: Query<Entity, With<ChoiceButton>>,
    mut index: ResMut<Selected_Choice_Index>,
    mut selected: ResMut<Selected_Choice>
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
                                /* Text::from_section(
                                    format!("{}: {}", dialogue.speaker, dialogue.text),
                                    TextStyle {
                                        font: asset_server.load("fonts/FiraSans-Bold.ttf"),
                                        font_size: 24.0,
                                        color: Color::WHITE,
                                    },
                                ) */
                               //.with_text_alignment(TextAlignment::Left),
                               Text::new(format!("{}: {}", dialogue.speaker, dialogue.text)),
                               TextFont {
                                        //font: asset_server.load("fonts/FiraSans-Bold.ttf"),
                                        font_size: 24.0,
                                        ..Default::default()
                                    },
                                TextColor(Color::WHITE),
                                Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
                                GlobalTransform::default(),
                            ),
                        );
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
                                            width: Val::Px(200.0),
                                            height: Val::Px(40.0),
                                            margin: UiRect::all(Val::Px(5.0)),
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            ..default()
                                        },
                                        BackgroundColor(if is_selected {
                                            Color::srgb(0.25, 0.4, 0.25) // highlighted color
                                        } else {
                                            Color::srgb(0.15, 0.3, 0.15) // normal color
                                        }),
                                        ChoiceButton {
                                            next_id: choice.next.as_ref().unwrap().clone(),
                                        },
                                    ))
                                    .with_children(|btn| {
                                        btn.spawn((
                                            Text::new(&choice.text),
                                            TextFont {
                                                font_size: 20.0,
                                                ..Default::default()
                                            },
                                            TextColor(if is_selected {
                                                Color::BLACK // highlighted text color
                                            } else {
                                                Color::WHITE // normal text color
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
            }
            else {
                println!("No dialogue box found, it has not spawned yet");
            }
            
        }
        else {
            println!("No dialogue found for ID: {}", current_id);
        }
    }
    else {
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

fn handle_choice_clicks(
    mut interaction_query: Query<(&Interaction, &ChoiceButton), (Changed<Interaction>, With<Button>)>,
    mut state: ResMut<Dialogue_State>,
) {
    for (interaction, choice) in interaction_query.iter_mut() {
        if *interaction == Interaction::Pressed {
            state.0.current_id = Some(choice.next_id.clone());
        }
    }
}

fn handle_choice_event(
    event: u32,
) {
    match event {
        0 => println!("Choice 1 selected"),
        1 => println!("Choice 2 selected"),
        _ => println!("Invalid choice"),
    }
}
