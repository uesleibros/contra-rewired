//! One file per `VERIFY_*` mode's `Ctx` struct + `verify_*` function -
//! the pieces that were already factored out of `main`'s own dispatch
//! chain (see `main.rs` for the dispatch itself, which still owns each
//! mode's hook-registration/entry-capture logic and just calls into
//! these on exit).

pub mod blue_soldier_routine_01;
pub mod blue_soldier_routine_02;
pub mod blue_soldier_routine_03;
pub mod enemy_routine_explosion;
pub mod enemy_routine_init_explosion;
pub mod enemy_routine_remove_enemy;
pub mod four_soldiers_routine_00;
pub mod four_soldiers_routine_01;
pub mod four_soldiers_routine_02;
pub mod grenade_launcher_routine_00;
pub mod grenade_launcher_routine_01;
pub mod indoor_roller_gen_routine_00;
pub mod indoor_roller_gen_routine_01;
pub mod indoor_soldier_gen_routine_00;
pub mod indoor_soldier_gen_routine_01;
pub mod indoor_soldier_routine_00;
pub mod indoor_soldier_routine_01;
pub mod jumping_soldier_routine_00;
pub mod jumping_soldier_routine_01;
pub mod red_blue_soldier_gen_routine_01;
pub mod red_blue_soldier_routine_00;
pub mod red_soldier_routine_01;
pub mod red_soldier_routine_02;
pub mod shared_enemy_routine_00;
pub mod shared_enemy_routine_01;
pub mod shared_enemy_routine_03;
pub mod soldier_routine_01;
pub mod soldier_routine_02_jumping;
pub mod soldier_routine_03;
pub mod soldier_routine_04;
pub mod soldier_routine_05;
pub mod soldier_routine_09;
pub mod soldier_routine_0a;

pub use blue_soldier_routine_01::*;
pub use blue_soldier_routine_02::*;
pub use blue_soldier_routine_03::*;
pub use enemy_routine_explosion::*;
pub use enemy_routine_init_explosion::*;
pub use enemy_routine_remove_enemy::*;
pub use four_soldiers_routine_00::*;
pub use four_soldiers_routine_01::*;
pub use four_soldiers_routine_02::*;
pub use grenade_launcher_routine_00::*;
pub use grenade_launcher_routine_01::*;
pub use indoor_roller_gen_routine_00::*;
pub use indoor_roller_gen_routine_01::*;
pub use indoor_soldier_gen_routine_00::*;
pub use indoor_soldier_gen_routine_01::*;
pub use indoor_soldier_routine_00::*;
pub use indoor_soldier_routine_01::*;
pub use jumping_soldier_routine_00::*;
pub use jumping_soldier_routine_01::*;
pub use red_blue_soldier_gen_routine_01::*;
pub use red_blue_soldier_routine_00::*;
pub use red_soldier_routine_01::*;
pub use red_soldier_routine_02::*;
pub use shared_enemy_routine_00::*;
pub use shared_enemy_routine_01::*;
pub use shared_enemy_routine_03::*;
pub use soldier_routine_01::*;
pub use soldier_routine_02_jumping::*;
pub use soldier_routine_03::*;
pub use soldier_routine_04::*;
pub use soldier_routine_05::*;
pub use soldier_routine_09::*;
pub use soldier_routine_0a::*;
