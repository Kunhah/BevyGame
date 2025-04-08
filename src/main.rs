use bevy::prelude::*;
use bevy::sprite::*;
use bevy::prelude::GltfAssetLabel::Texture;
use bevy::input::keyboard::KeyCode; // KeyCode fix
use bevy::math::Rect;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use once_cell::sync::Lazy;

const WINDOW_WIDTH: f32 = 640.0;
const WINDOW_HEIGHT: f32 = 480.0;
const PLAYER_SPEED: f32 = 200.0;

const GRID_WIDTH: usize = 200;
const GRID_HEIGHT: usize = 150;

static GAME_STATE: Lazy<Arc<Mutex<GameState>>> = Lazy::new(|| {
    Arc::new(Mutex::new(GameState {
        interacting: false,
        battle: false
    }))
});

struct GameState {
    interacting: bool,
    battle: bool
}

// New component to mark walls
#[derive(Component)]
struct Collider{
    name: String,
}

#[derive(Component, Clone)]
struct Interactable {
    name: String,
}

impl Interactable {
    fn interact(&self, transform: &bevy::prelude::Transform, mut game_state: MutexGuard<'_, GameState>) {
        // code to handle interaction goes here
        game_state.interacting = true;
        println!("Interacting is: {}", game_state.interacting);
        game_state.interacting = false;
        println!("Interacting is: {}", game_state.interacting);
        println!("Interacted with {}, on transform: {:?}", self.name, transform);
    }
}

/* // New resource to keep track of the collision grid
#[derive(Resource)]
struct CollisionGrid(Vec<Vec<bool>>); */


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
        .add_systems(Startup, setup)
        .add_systems(Update, player_movement)
        .add_systems(Update, interact)
        .run();
}

#[derive(Component)]
struct Player;

#[derive(Component)]
struct Position {
    x: i32,
    y: i32,
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    
    commands
    .spawn(Camera2d::default())
    .insert(Player)
    .insert(Position { x: 0, y: 0 });

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
            Position { x: 0, y: 0 },
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
            Position { x: 0, y: 0 },
            Interactable { name: "Test interactable".to_string() },
        ));
    }

}

fn player_movement(
    /* mut query: Query<(&mut Transform, &mut Position), With<Player>>,
    collider_query: Query<&Transform, With<Collider>>, */
    mut param_set: ParamSet<(
        Query<(&mut Transform, &mut Position), With<Player>>,
        Query<&Transform, With<Collider>>,
    )>,
    input: Res<ButtonInput<KeyCode>>, 
    time: Res<Time>,
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
        if direction.x != 0.0 && direction.y != 0.0 {
            // Diagonal movement
            let diagonal_speed = movement_speed / (2.0_f32.sqrt());
             // First, collect all collider data safely
            let p1 = param_set.p1();
            let colliders: Vec<_> = p1.iter().cloned().collect();
            //let colliders: Vec<_> = p1.iter().collect();

            let mut p0 = param_set.p0();

            for (mut transform, mut position) in p0.iter_mut() {
                let new_x = transform.translation.x + direction.x * diagonal_speed;
                let new_y = transform.translation.y + direction.y * diagonal_speed;

                // Check if the new position is within the collision grid
                let grid_x = (new_x / 32.0) as usize;
                let grid_y = (new_y / 32.0) as usize;

                if grid_x < GRID_WIDTH && grid_y < GRID_HEIGHT {
                    /* transform.translation.x = new_x;
                    transform.translation.y = new_y;
                    position.x = (new_x / 32.0) as i32;
                    position.y = (new_y / 32.0) as i32; */

                     // Collision detection
                    let player_rect = Rect::from_center_size(Vec2::new(new_x, new_y), Vec2::new(32.0, 32.0));

                    // Check collision using AABB
                    let collision = colliders.iter().any(|wall_transform| {
                        let wall_rect = Rect::from_center_size(
                            Vec2::new(wall_transform.translation.x, wall_transform.translation.y),
                            Vec2::new(32.0, 32.0),
                        );
                        aabb_collision(player_rect, wall_rect)
                    });

                    if !collision {
                        transform.translation.x = new_x;
                        transform.translation.y = new_y;
                        position.x = (new_x / 32.0) as i32;
                        position.y = (new_y / 32.0) as i32;
                    }
                }
            }
        } else {

            let p1 = param_set.p1();
            let colliders: Vec<_> = p1.iter().cloned().collect();
            // Horizontal or vertical movement
            for (mut transform, mut position) in param_set.p0().iter_mut() {
                let new_x = transform.translation.x + direction.x * movement_speed;
                let new_y = transform.translation.y + direction.y * movement_speed;

                // Check if the new position is within the collision grid
                let grid_x = (new_x / 32.0) as usize;
                let grid_y = (new_y / 32.0) as usize;

                if grid_x < GRID_WIDTH && grid_y < GRID_HEIGHT {
                    /* transform.translation.x = new_x;
                    transform.translation.y = new_y;
                    position.x = (new_x / 32.0) as i32;
                    position.y = (new_y / 32.0) as i32; */

                    let player_rect = Rect::from_center_size(Vec2::new(new_x, new_y), Vec2::new(32.0, 32.0));

                    // Check collision using AABB
                    let collision = colliders.iter().any(|wall_transform| {
                        let wall_rect = Rect::from_center_size(
                            Vec2::new(wall_transform.translation.x, wall_transform.translation.y),
                            Vec2::new(32.0, 32.0),
                        );
                        aabb_collision(player_rect, wall_rect)
                    });

                    if !collision {
                        transform.translation.x = new_x;
                        transform.translation.y = new_y;
                        position.x = (new_x / 32.0) as i32;
                        position.y = (new_y / 32.0) as i32;
                    }
                }
            }
        }
    }
}

fn interact<'a>(
    mut param_set: ParamSet<(
        Query<(&Transform, &Position), With<Player>>,
        Query<(&Transform, &Interactable), With<Interactable>>,
        //Query<&Interactable, With<Interactable>>
    )>,
    input: Res<ButtonInput<KeyCode>>, 
) {
    let game_state = GAME_STATE.lock().unwrap();
    if (!input.pressed(KeyCode::KeyX) || game_state.interacting || game_state.battle) {
        return;
    }
    
    let interactables: Vec<(Transform, Interactable)> = param_set
        .p1()
        .iter()
        .map(|(t, i)| (t.clone(), i.clone())) 
        .collect();

    //let interactables: Vec<(Rc<&Transform>, Rc<&Interactable>)> = p1.iter().map(|(t, i)| (Rc::new(t), Rc::new(i))).collect();
    
    let p0 = param_set.p0();

    for (transform, position) in p0.iter() {

        let player_rect = Rect::from_center_size(Vec2::new(transform.translation.x, transform.translation.y), Vec2::new(32.0, 32.0));

        let interactable: Option<&(bevy::prelude::Transform, Interactable)> = interactables.iter().find(|(interactable_transform, interactable)| {
            let wall_rect = Rect::from_center_size(
                Vec2::new(interactable_transform.translation.x, interactable_transform.translation.y),
                Vec2::new(32.0, 32.0),
            );
            aabb_collision(player_rect, wall_rect)
        });

        if let Some((interactable_transform, interactable)) = interactable {
            interactable.interact(interactable_transform, game_state);
            break;
        }
    }
}

fn aabb_collision(rect1: Rect, rect2: Rect) -> bool {
    rect1.min.x < rect2.max.x &&
    rect1.max.x > rect2.min.x &&
    rect1.min.y < rect2.max.y &&
    rect1.max.y > rect2.min.y
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
                    position.x = (new_x / 32.0) as i32;
                    position.y = (new_y / 32.0) as i32;
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
                    position.x = (new_x / 32.0) as i32;
                    position.y = (new_y / 32.0) as i32;
                }
            }
        }
    }
} */