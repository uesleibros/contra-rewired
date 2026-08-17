/// Captured inputs for one real `red_blue_soldier_gen_routine_01` call
/// (`$a309`). See `VERIFY_RED_BLUE_SOLDIER_GEN_ROUTINE_01`'s comment in
/// `main` for the real exits, the bank gate, and the nested-return
/// disambiguation this needs.
#[derive(Clone)]
pub struct RedBlueSoldierGenRoutine01Ctx {
    pub x: usize,
    pub current_level: u8,
    pub wall_plating_destroyed_count: u8,
    pub frame_counter: u8,
    pub animation_delay: u8,
    pub var_1: u8,
    pub enemy_routine: [u8; contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT],
}

pub fn verify_red_blue_soldier_gen_routine_01(
    ctx: RedBlueSoldierGenRoutine01Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::red_blue_soldier::{red_blue_soldier_gen_routine_01, RedBlueSoldierGenRoutine01Outcome};

    let x = ctx.x;
    let expected = red_blue_soldier_gen_routine_01(
        prg_rom, &ctx.enemy_routine, ctx.current_level, ctx.wall_plating_destroyed_count, ctx.frame_counter,
        ctx.animation_delay, ctx.var_1,
    );
    *checked += 1;

    let real_delay = bus.ram[0x538 + x];
    let real_var_1 = bus.ram[0x5B8 + x];
    let real_routine = bus.ram[0x4B8 + x];
    let real_sprites = bus.ram[0x30A + x];

    let mismatch = match expected.outcome {
        RedBlueSoldierGenRoutine01Outcome::Removed(_) => real_routine != 0 || real_sprites != 0,
        RedBlueSoldierGenRoutine01Outcome::OddFrame => real_delay != ctx.animation_delay,
        RedBlueSoldierGenRoutine01Outcome::StillWaiting { animation_delay } => real_delay != animation_delay,
        RedBlueSoldierGenRoutine01Outcome::Spawned { var_1, animation_delay } => {
            let mut m = real_var_1 != var_1 || real_delay != animation_delay;
            for spawn in &expected.spawns {
                let s = spawn.slot as usize;
                m |= bus.ram[0x528 + s] != spawn.enemy_type
                    || bus.ram[0x5A8 + s] != spawn.attributes
                    || bus.ram[0x4B8 + s] != spawn.initialized.routine;
            }
            m
        }
    };

    if mismatch {
        eprintln!(
            "MISMATCH(red_blue_soldier_gen_routine_01) frame={frame} pc={:04X} in=(wall_plating={:02X} frame_counter={:02X} delay={:02X} var_1={:02X}): expected {:?}, got delay={real_delay:02X} var_1={real_var_1:02X} routine={real_routine:02X} sprites={real_sprites:02X}",
            cpu.pc, ctx.wall_plating_destroyed_count, ctx.frame_counter, ctx.animation_delay, ctx.var_1, expected
        );
    }
}
