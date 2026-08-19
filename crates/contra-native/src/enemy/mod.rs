//! Enemy AI, spawning, and shared enemy-lifecycle logic - everything
//! that operates on the 16-slot enemy array (`ENEMY_ROUTINE`, `ENEMY_TYPE`,
//! etc.) or a specific enemy type's own routines (e.g. [`soldier`]).

pub mod add_scroll_to_enemy_pos;
pub mod add_with_enemy_pos;
pub mod claw;
pub mod create_enemy_bullet;
pub mod enemy_bullet;
pub mod enemy_clear;
pub mod enemy_collision_flags;
pub mod enemy_explosion;
pub mod enemy_falling_arc;
pub mod enemy_position_utils;
pub mod enemy_routine_transition;
pub mod enemy_slots;
pub mod enemy_spawn;
pub mod find_far_segment;
pub mod flying_capsule;
pub mod four_soldiers;
pub mod generate_enemy_at_pos;
pub mod grenade;
pub mod grenade_launcher;
pub mod ice;
pub mod indoor_roller_gen;
pub mod indoor_soldier;
pub mod indoor_soldier_gen;
pub mod initialize_enemy;
pub mod jumping_soldier;
pub mod mine_cart;
pub mod mortar_shot;
pub mod player_enemy_distance;
pub mod quadrant_aim_dir;
pub mod red_blue_soldier;
pub mod rock;
pub mod roller;
pub mod scuba_soldier;
pub mod sniper;
pub mod soldier;
pub mod spiked_wall;
pub mod turret_man;
pub mod ufo;
pub mod update_enemy_pos;
pub mod weapon_item;
