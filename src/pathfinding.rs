use std::cmp::Ordering;
use std::collections::BinaryHeap;

use bevy::prelude::*;

use crate::constants::{GRID_HEIGHT, GRID_WIDTH, WALKING_LIMIT};
use crate::core::Position;
use crate::quadtree::{aabb_collision, Collider, QuadTree};

const PATH_DIRECTIONS: [(i32, i32); 8] = [
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
];
const LOCAL_GRID_PADDING_STEPS: i32 = 24;
const WALKABLE_UNKNOWN: u8 = 0;
const WALKABLE_BLOCKED: u8 = 1;
const WALKABLE_OPEN: u8 = 2;

#[derive(Copy, Clone, Eq, PartialEq)]
struct Node_P {
    index: usize,
    cost: i32,
    priority: i32,
}

impl Ord for Node_P {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .priority
            .cmp(&self.priority)
            .then_with(|| other.cost.cmp(&self.cost))
    }
}

impl PartialOrd for Node_P {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(
            other
                .priority
                .cmp(&self.priority)
                .then_with(|| other.cost.cmp(&self.cost)),
        )
    }
}

struct LocalGrid {
    start: Position,
    margin: i32,
    min_step_x: i32,
    max_step_x: i32,
    min_step_y: i32,
    max_step_y: i32,
    width: usize,
    height: usize,
}

impl LocalGrid {
    fn new(start: Position, goal: Position, margin: i32) -> Self {
        let max_reach = WALKING_LIMIT as i32 + LOCAL_GRID_PADDING_STEPS;
        let goal_step_x = signed_ceil_div(goal.x - start.x, margin).clamp(-max_reach, max_reach);
        let goal_step_y = signed_ceil_div(goal.y - start.y, margin).clamp(-max_reach, max_reach);

        let min_step_x = (goal_step_x.min(0) - LOCAL_GRID_PADDING_STEPS).max(-max_reach);
        let max_step_x = (goal_step_x.max(0) + LOCAL_GRID_PADDING_STEPS).min(max_reach);
        let min_step_y = (goal_step_y.min(0) - LOCAL_GRID_PADDING_STEPS).max(-max_reach);
        let max_step_y = (goal_step_y.max(0) + LOCAL_GRID_PADDING_STEPS).min(max_reach);
        let width = (max_step_x - min_step_x + 1) as usize;
        let height = (max_step_y - min_step_y + 1) as usize;

        Self {
            start,
            margin,
            min_step_x,
            max_step_x,
            min_step_y,
            max_step_y,
            width,
            height,
        }
    }

    /// A grid centred on `start`, reaching `radius_steps` in every direction.
    /// Used by the reachability flood ([`reachable_tiles`]), which has no goal to
    /// bias the bounds toward — unlike [`LocalGrid::new`], which leans the box in
    /// the goal's direction.
    fn centered(start: Position, radius_steps: i32, margin: i32) -> Self {
        let min_step_x = -radius_steps;
        let max_step_x = radius_steps;
        let min_step_y = -radius_steps;
        let max_step_y = radius_steps;
        let width = (max_step_x - min_step_x + 1) as usize;
        let height = (max_step_y - min_step_y + 1) as usize;

        Self {
            start,
            margin,
            min_step_x,
            max_step_x,
            min_step_y,
            max_step_y,
            width,
            height,
        }
    }

    fn len(&self) -> usize {
        self.width * self.height
    }

    fn index(&self, step_x: i32, step_y: i32) -> Option<usize> {
        if step_x < self.min_step_x
            || step_x > self.max_step_x
            || step_y < self.min_step_y
            || step_y > self.max_step_y
        {
            return None;
        }

        let local_x = (step_x - self.min_step_x) as usize;
        let local_y = (step_y - self.min_step_y) as usize;
        Some(local_x + local_y * self.width)
    }

    fn step_coords(&self, index: usize) -> (i32, i32) {
        let local_x = (index % self.width) as i32;
        let local_y = (index / self.width) as i32;

        (self.min_step_x + local_x, self.min_step_y + local_y)
    }

    fn position(&self, index: usize) -> Position {
        let (step_x, step_y) = self.step_coords(index);
        Position {
            x: self.start.x + step_x * self.margin,
            y: self.start.y + step_y * self.margin,
        }
    }
}

fn signed_ceil_div(value: i32, divisor: i32) -> i32 {
    if value >= 0 {
        (value + divisor - 1) / divisor
    } else {
        -((-value + divisor - 1) / divisor)
    }
}

fn distance(a: Position, b: Position) -> i32 {
    let dx = (a.x - b.x).abs();
    let dy = (a.y - b.y).abs();
    let diagonal = dx.min(dy);
    let straight = dx.max(dy) - diagonal;

    diagonal * 14 + straight * 10
}

fn walkable_query<'a>(
    pos: Position,
    quad_tree: &'a QuadTree,
    possible_colliders: &mut Vec<&'a Collider>,
) -> bool {
    if pos.x.abs() as u32 > GRID_WIDTH || pos.y.abs() as u32 > GRID_HEIGHT {
        return false;
    }

    let pos_center = Vec2::new(pos.x as f32, pos.y as f32);
    let player_rect = Rect::from_center_size(pos_center, Vec2::new(32.0, 32.0));

    possible_colliders.clear();
    quad_tree.0.query(player_rect, possible_colliders);

    !possible_colliders
        .iter()
        .any(|collider| aabb_collision(player_rect, collider.bounds))
}

pub fn is_walkable_move(pos: Position, quad_tree: &QuadTree) -> bool {
    let mut possible_colliders = Vec::with_capacity(16);
    walkable_query(pos, quad_tree, &mut possible_colliders)
}

pub fn is_walkable_path(pos: Position, quad_tree: &QuadTree) -> bool {
    let mut possible_colliders = Vec::with_capacity(16);
    walkable_query(pos, quad_tree, &mut possible_colliders)
}

pub fn pathfinding(
    quad_tree: &QuadTree,
    start: Position,
    goal: Position,
    margin: i32,
) -> Vec<Position> {
    let mut possible_colliders = Vec::with_capacity(16);
    if !walkable_query(start, quad_tree, &mut possible_colliders)
        || !walkable_query(goal, quad_tree, &mut possible_colliders)
    {
        return Vec::new();
    }

    let grid = LocalGrid::new(start, goal, margin);
    let Some(start_index) = grid.index(0, 0) else {
        return Vec::new();
    };

    let mut open_set = BinaryHeap::new();
    open_set.push(Node_P {
        index: start_index,
        cost: 0,
        priority: distance(start, goal),
    });

    let cell_count = grid.len();
    let mut came_from = vec![None; cell_count];
    let mut g_score = vec![i32::MAX; cell_count];
    let mut closed = vec![false; cell_count];
    let mut walkable_cache = vec![WALKABLE_UNKNOWN; cell_count];
    g_score[start_index] = 0;
    walkable_cache[start_index] = WALKABLE_OPEN;

    let mut best_index = start_index;
    let mut best_goal_distance = distance(start, goal);
    let mut expanded_nodes = 0usize;

    while let Some(current_node) = open_set.pop() {
        if current_node.cost > g_score[current_node.index] || closed[current_node.index] {
            continue;
        }
        closed[current_node.index] = true;

        let current_position = grid.position(current_node.index);
        let current_goal_distance = distance(current_position, goal);
        if current_goal_distance < best_goal_distance {
            best_goal_distance = current_goal_distance;
            best_index = current_node.index;
        }

        if current_goal_distance < margin * 10 {
            best_index = current_node.index;
            break;
        }

        expanded_nodes += 1;
        if expanded_nodes > 1000 {
            break;
        }

        let (current_step_x, current_step_y) = grid.step_coords(current_node.index);
        for (dx, dy) in PATH_DIRECTIONS {
            let neighbor_step_x = current_step_x + dx;
            let neighbor_step_y = current_step_y + dy;
            let Some(neighbor_index) = grid.index(neighbor_step_x, neighbor_step_y) else {
                continue;
            };
            if closed[neighbor_index] {
                continue;
            }

            if walkable_cache[neighbor_index] == WALKABLE_UNKNOWN {
                let neighbor = grid.position(neighbor_index);
                walkable_cache[neighbor_index] = if walkable_query(
                    neighbor,
                    quad_tree,
                    &mut possible_colliders,
                ) {
                    WALKABLE_OPEN
                } else {
                    WALKABLE_BLOCKED
                };
            }
            if walkable_cache[neighbor_index] == WALKABLE_BLOCKED {
                continue;
            }

            let movement_cost = if dx == 0 || dy == 0 { 10 } else { 14 };
            let tentative_g = current_node.cost + movement_cost;

            if tentative_g < g_score[neighbor_index] {
                came_from[neighbor_index] = Some(current_node.index);
                g_score[neighbor_index] = tentative_g;

                let priority = tentative_g + distance(grid.position(neighbor_index), goal);
                open_set.push(Node_P {
                    index: neighbor_index,
                    cost: tentative_g,
                    priority,
                });
            }
        }
    }

    let mut path = vec![grid.position(best_index)];
    let mut curr = best_index;
    while let Some(prev) = came_from[curr] {
        path.push(grid.position(prev));
        curr = prev;
    }
    path.reverse();

    path
}

/// Flood every cell reachable from `start` whose accumulated travel cost stays
/// within `budget` **world units**, respecting obstacles via the same collider
/// walkability test [`pathfinding`] uses. This is the classic tactics-game
/// "movement range": every spot the unit could step onto this turn.
///
/// This is **Dijkstra**, not A\*. There is no goal, so there is no heuristic to
/// steer the search — we just expand outward in cheapest-cost-first order until
/// the budget runs out. (Concretely it *is* A\* with the heuristic forced to
/// zero: `priority == cost`, see the push below.) Dijkstra — rather than a plain
/// BFS — is required because diagonal steps cost more than cardinal ones, so the
/// cheapest way to a cell isn't always the fewest steps.
///
/// Returns each reachable cell paired with the world-unit cost to reach it — a
/// distance field, so a caller can colour by remaining range or read a path back
/// out without a second search. `margin` is the grid step in world units
/// (coarser = cheaper to compute and blockier to look at).
pub fn reachable_tiles(
    quad_tree: &QuadTree,
    start: Position,
    budget: f32,
    margin: i32,
) -> Vec<(Position, f32)> {
    if budget <= 0.0 || margin <= 0 {
        return Vec::new();
    }

    let mut possible_colliders = Vec::with_capacity(16);
    if !walkable_query(start, quad_tree, &mut possible_colliders) {
        return Vec::new();
    }

    // Costs use the same 10-per-cardinal / 14-per-diagonal scale as A*'s
    // `distance`, so one step of `margin` world units == 10 cost units. Convert
    // the world-unit budget into that scale and size the grid to match (clamped
    // to the same reach ceiling the planner uses, so a huge budget can't blow up
    // the allocation).
    let steps_budget = budget / margin as f32;
    let budget_cost = (steps_budget * 10.0).round() as i32;
    let max_reach = WALKING_LIMIT as i32 + LOCAL_GRID_PADDING_STEPS;
    let radius_steps = (steps_budget.ceil() as i32 + 1).clamp(1, max_reach);

    let grid = LocalGrid::centered(start, radius_steps, margin);
    let Some(start_index) = grid.index(0, 0) else {
        return Vec::new();
    };

    let cell_count = grid.len();
    let mut g_score = vec![i32::MAX; cell_count];
    let mut closed = vec![false; cell_count];
    let mut walkable_cache = vec![WALKABLE_UNKNOWN; cell_count];

    let mut open_set = BinaryHeap::new();
    g_score[start_index] = 0;
    walkable_cache[start_index] = WALKABLE_OPEN;
    // Dijkstra == A* with a zero heuristic: the priority *is* the cost so far.
    open_set.push(Node_P {
        index: start_index,
        cost: 0,
        priority: 0,
    });

    let mut reachable = Vec::new();

    while let Some(current) = open_set.pop() {
        if current.cost > g_score[current.index] || closed[current.index] {
            continue;
        }
        closed[current.index] = true;
        reachable.push((
            grid.position(current.index),
            current.cost as f32 * margin as f32 / 10.0,
        ));

        let (current_step_x, current_step_y) = grid.step_coords(current.index);
        for (dx, dy) in PATH_DIRECTIONS {
            let Some(neighbor_index) = grid.index(current_step_x + dx, current_step_y + dy) else {
                continue;
            };
            if closed[neighbor_index] {
                continue;
            }

            if walkable_cache[neighbor_index] == WALKABLE_UNKNOWN {
                let neighbor = grid.position(neighbor_index);
                walkable_cache[neighbor_index] =
                    if walkable_query(neighbor, quad_tree, &mut possible_colliders) {
                        WALKABLE_OPEN
                    } else {
                        WALKABLE_BLOCKED
                    };
            }
            if walkable_cache[neighbor_index] == WALKABLE_BLOCKED {
                continue;
            }

            let movement_cost = if dx == 0 || dy == 0 { 10 } else { 14 };
            let tentative_g = current.cost + movement_cost;
            if tentative_g <= budget_cost && tentative_g < g_score[neighbor_index] {
                g_score[neighbor_index] = tentative_g;
                open_set.push(Node_P {
                    index: neighbor_index,
                    cost: tentative_g,
                    priority: tentative_g,
                });
            }
        }
    }

    reachable
}
