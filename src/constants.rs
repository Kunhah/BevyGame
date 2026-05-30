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
//
// One tick is 2^3 = 8 seconds, chosen as a power of two so seconds<->ticks is a
// pure bit shift: `seconds = ticks << 3`, `ticks = seconds >> 3`. Keep
// `TIMESTAMP_SECONDS_PER_TICK` defined via the shift so the two never drift.
pub const TIMESTAMP_TICK_SHIFT: u32 = 3;
pub const TIMESTAMP_SECONDS_PER_TICK: u32 = 1 << TIMESTAMP_TICK_SHIFT; // 8

/// Real seconds -> ticks (right shift by `TIMESTAMP_TICK_SHIFT`).
#[inline]
pub const fn seconds_to_ticks(seconds: u32) -> u32 {
    seconds >> TIMESTAMP_TICK_SHIFT
}

/// Ticks -> real seconds (left shift by `TIMESTAMP_TICK_SHIFT`).
#[inline]
pub const fn ticks_to_seconds(ticks: u32) -> u32 {
    ticks << TIMESTAMP_TICK_SHIFT
}

// 8 does not divide 60 evenly, so `TICKS_PER_MINUTE` truncates (7.5 -> 7) and
// must NOT be used to build `TICKS_PER_HOUR`; the hour is taken from 3600 s
// directly (3600 >> 3 = 450 ticks/hour, exact).
pub const TIMESTAMP_TICKS_PER_MINUTE: u32 = seconds_to_ticks(60);
pub const TIMESTAMP_TICKS_PER_HOUR: u32 = seconds_to_ticks(3600);
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
