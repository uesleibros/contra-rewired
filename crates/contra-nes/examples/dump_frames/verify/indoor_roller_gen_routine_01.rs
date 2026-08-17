/// Captured inputs for one real `indoor_roller_gen_routine_01` call
/// (`$95cd`).
#[derive(Clone)]
pub struct IndoorRollerGenRoutine01Ctx {
    pub x: usize,
    pub current_level: u8,
    pub attack_flag: u8,
    pub indoor_enemy_attack_count: u8,
    pub frame_counter: u8,
    pub animation_delay: u8,
    pub attributes: u8,
    pub var_1: u8,
    pub enemy_routine: [u8; contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT],
}

pub fn verify_indoor_roller_gen_routine_01(
    ctx: IndoorRollerGenRoutine01Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::indoor_roller_gen::{indoor_roller_gen_routine_01, IndoorRollerGenRoutine01Outcome};

    let x = ctx.x;
    let expected = indoor_roller_gen_routine_01(
        prg_rom,
        &ctx.enemy_routine,
        ctx.current_level,
        ctx.attack_flag,
        ctx.indoor_enemy_attack_count,
        ctx.frame_counter,
        ctx.animation_delay,
        ctx.attributes,
        ctx.var_1,
    );
    *checked += 1;

    let real_animation_delay = bus.ram[0x538 + x];
    let real_var_1 = bus.ram[0x5B8 + x];
    let real_routine = bus.ram[0x4B8 + x];
    let real_sprites = bus.ram[0x30A + x];

    let mismatch = match &expected {
        IndoorRollerGenRoutine01Outcome::RoundsExhausted(_) => real_routine != 0 || real_sprites != 0,
        IndoorRollerGenRoutine01Outcome::EvenFrame => false,
        IndoorRollerGenRoutine01Outcome::StillWaiting { animation_delay } => real_animation_delay != *animation_delay,
        IndoorRollerGenRoutine01Outcome::Entry(result) => {
            let mut m = real_animation_delay != result.animation_delay || real_var_1 != result.var_1;
            for spawn in &result.spawns {
                if let Some(roller) = &spawn.roller {
                    let s = roller.slot as usize;
                    m |= bus.ram[0x528 + s] != roller.enemy_type
                        || bus.ram[0x5A8 + s] != roller.fields.attributes
                        || bus.ram[0x33E + s] != roller.fields.x_pos
                        || bus.ram[0x324 + s] != roller.fields.y_pos
                        || bus.ram[0x4B8 + s] == 0;
                }
            }
            m
        }
    };

    if mismatch {
        eprintln!(
            "MISMATCH(indoor_roller_gen_routine_01) frame={frame} pc={:04X} in=(attack_count={:02X} frame_counter={:02X} delay={:02X} attrs={:02X} var_1={:02X}): expected {:?}, got animation_delay={real_animation_delay:02X} var_1={real_var_1:02X} routine={real_routine:02X} sprites={real_sprites:02X}",
            cpu.pc, ctx.indoor_enemy_attack_count, ctx.frame_counter, ctx.animation_delay, ctx.attributes, ctx.var_1, expected
        );
    }
}
