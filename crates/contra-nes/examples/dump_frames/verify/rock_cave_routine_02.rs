use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

/// Captured inputs for one real `rock_cave_routine_02` call (`$986b`).
#[derive(Clone, Copy)]
pub struct RockCaveRoutine02Ctx {
    pub x: usize,
    pub current_level: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub x_vel_accum: u8,
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    pub y_pos: u8,
    pub y_vel_accum: u8,
    pub y_vel_fract: u8,
    pub y_vel_fast: u8,
    pub animation_delay: u8,
    pub enemy_routine_slots: [u8; ENEMY_SLOT_COUNT],
}

pub fn verify_rock_cave_routine_02(
    ctx: RockCaveRoutine02Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::rock::{rock_cave_routine_02, RockCaveRoutine02Outcome};

    let x = ctx.x;
    let expected = rock_cave_routine_02(
        prg_rom,
        &ctx.enemy_routine_slots,
        ctx.current_level,
        ctx.level_scrolling_type,
        ctx.frame_scroll,
        ctx.x_pos,
        ctx.x_vel_accum,
        ctx.x_vel_fract,
        ctx.x_vel_fast,
        ctx.y_pos,
        ctx.y_vel_accum,
        ctx.y_vel_fract,
        ctx.y_vel_fast,
        ctx.animation_delay,
    );
    *checked += 1;

    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_animation_delay = bus.ram[0x538 + x];

    let pos_ok = real_x_pos == expected.position.x.pos && real_y_pos == expected.position.y.pos;

    let outcome_ok = match &expected.outcome {
        RockCaveRoutine02Outcome::Waiting { animation_delay } => real_animation_delay == *animation_delay,
        RockCaveRoutine02Outcome::Spawned { animation_delay, rock } => {
            let rock_ok = match rock {
                Some(r) => {
                    let rx = r.slot as usize;
                    bus.ram[0x33E + rx] == r.x_pos && bus.ram[0x324 + rx] == r.y_pos && bus.ram[0x528 + rx] == 0x13 && bus.ram[0x4B8 + rx] == r.initialized.routine
                }
                None => true,
            };
            rock_ok && real_animation_delay == *animation_delay
        }
    };

    let mismatch = !pos_ok || !outcome_ok;

    if mismatch {
        eprintln!(
            "MISMATCH(rock_cave_routine_02) frame={frame} pc={:04X} in=(x={:02X} y={:02X} delay={:02X}): expected {:?}, got x={real_x_pos:02X} y={real_y_pos:02X} animation_delay={real_animation_delay:02X}",
            cpu.pc, ctx.x_pos, ctx.y_pos, ctx.animation_delay, expected.outcome
        );
    }
}
