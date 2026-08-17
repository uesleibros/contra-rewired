//! One file per `VERIFY_*` mode's `Ctx` struct + `verify_*` function -
//! the pieces that were already factored out of `main`'s own dispatch
//! chain (see `main.rs` for the dispatch itself, which still owns each
//! mode's hook-registration/entry-capture logic and just calls into
//! these on exit).

pub mod enemy_routine_explosion;
pub mod enemy_routine_init_explosion;
pub mod enemy_routine_remove_enemy;
pub mod soldier_routine_01;
pub mod soldier_routine_02_jumping;
pub mod soldier_routine_03;
pub mod soldier_routine_04;
pub mod soldier_routine_05;
pub mod soldier_routine_09;
pub mod soldier_routine_0a;

pub use enemy_routine_explosion::*;
pub use enemy_routine_init_explosion::*;
pub use enemy_routine_remove_enemy::*;
pub use soldier_routine_01::*;
pub use soldier_routine_02_jumping::*;
pub use soldier_routine_03::*;
pub use soldier_routine_04::*;
pub use soldier_routine_05::*;
pub use soldier_routine_09::*;
pub use soldier_routine_0a::*;
