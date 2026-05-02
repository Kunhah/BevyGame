use bitflags::bitflags;

pub const SEGMENT_SIZE: f32 = 8.0;
pub const LIGHT_ALPHA: f32 = 0.15;
pub const LIGHT_WIDTH: f32 = 2.0;
pub const NOISE_SCALE: f64 = 0.2;
pub const MAX_DISTANCE_LIGHT: f32 = 200.0;

pub const MAX_DISTANCE_RENDER: f32 = 1000.0;
pub const MAX_DISTANCE_COLLISION: f32 = 10.0;

pub const MAX_OBJECTS: usize = 4;
pub const MAX_LEVELS: usize = 5;

pub const WINDOW_WIDTH: f32 = 1920.0;
pub const WINDOW_HEIGHT: f32 = 1080.0;
pub const PLAYER_SPEED: f32 = 200.0;

pub const GRID_WIDTH: u32 = 15000;
pub const GRID_HEIGHT: u32 = 15000;

pub const PATH_MARGIN: i32 = 5;
pub const PATH_DRAW_MARGIN: i32 = 4;
pub const PATH_MOVEMENT_SPEED: u32 = 20;
pub const DEFAULT_ACTION_POINTS: i32 = 8;
pub const BASIC_ATTACK_ACTION_POINT_COST: i32 = 4;
pub const ITEM_ACTION_POINT_COST: i32 = 4;

pub const WALKING_LIMIT: usize = 600 / PATH_DRAW_MARGIN as usize;

// Canonical world-time conversion for Timestamp.
pub const TIMESTAMP_SECONDS_PER_TICK: u32 = 9;
pub const TIMESTAMP_TICKS_PER_MINUTE: u32 = 60 / TIMESTAMP_SECONDS_PER_TICK;
pub const TIMESTAMP_TICKS_PER_HOUR: u32 = 60 * TIMESTAMP_TICKS_PER_MINUTE;
pub const DEFAULT_MAGIC_REGEN_PER_TICK: f32 =
    1.0 / ((4.0 * 60.0 * 60.0) / TIMESTAMP_SECONDS_PER_TICK as f32);

bitflags! {
    pub struct Flags: u128 {
        const FLAG1 = 1 << 0;
        const FLAG2 = 1 << 1;
        const FLAG3 = 1 << 2;
        const FLAG4 = 1 << 3;
    }
}
