use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

/// Captured inputs for one real `mine_cart_generator_routine_01` call
/// (`$b12c`).
#[derive(Clone, Copy)]
pub struct MineCartGeneratorRoutine01Ctx {
    pub x: usize,
    pub current_level: u8,
    pub enemy_frame: u8,
    pub animation_delay: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub enemy_routine_slots: [u8; ENEMY_SLOT_COUNT],
}

pub fn verify_mine_cart_generator_routine_01(
    ctx: MineCartGeneratorRoutine01Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::mine_cart::{mine_cart_generator_routine_01, MineCartGeneratorRoutine01Outcome};

    let x = ctx.x;
    let expected = mine_cart_generator_routine_01(
        prg_rom,
        &ctx.enemy_routine_slots,
        ctx.current_level,
        ctx.enemy_frame,
        ctx.animation_delay,
        ctx.level_scrolling_type,
        ctx.frame_scroll,
        ctx.x_pos,
        ctx.y_pos,
    );
    *checked += 1;

    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_frame = bus.ram[0x568 + x];
    let real_animation_delay = bus.ram[0x538 + x];

    let pos_ok = real_x_pos == expected.scroll.x_pos && real_y_pos == expected.scroll.y_pos;

    let outcome_ok = match &expected.outcome {
        MineCartGeneratorRoutine01Outcome::Waiting { animation_delay } => real_animation_delay == *animation_delay,
        MineCartGeneratorRoutine01Outcome::NoSlotAvailable { animation_delay } => real_animation_delay == *animation_delay,
        MineCartGeneratorRoutine01Outcome::Spawned { animation_delay, frame, cart } => {
            let cx = cart.slot as usize;
            real_animation_delay == *animation_delay
                && real_frame == *frame
                && bus.ram[0x33E + cx] == cart.x_pos
                && bus.ram[0x508 + cx] == cart.x_vel_fast
                && bus.ram[0x518 + cx] == cart.init.x_vel_fract
                && bus.ram[0x5E8 + cx] == cart.var_4
                && bus.ram[0x5A8 + cx] == cart.attributes
                && bus.ram[0x30A + cx] == cart.init.sprite
                && bus.ram[0x324 + cx] == cart.init.y_pos
                && bus.ram[0x528 + cx] == 0x14
                && bus.ram[0x4B8 + cx] == cart.initialized.routine
        }
        MineCartGeneratorRoutine01Outcome::CartStillAlive => true,
        MineCartGeneratorRoutine01Outcome::CartDestroyed { frame, animation_delay } => real_frame == *frame && real_animation_delay == *animation_delay,
    };

    let mismatch = !pos_ok || !outcome_ok;

    if mismatch {
        eprintln!(
            "MISMATCH(mine_cart_generator_routine_01) frame={frame} pc={:04X} in=(enemy_frame={:02X} delay={:02X} x={:02X} y={:02X}): expected {:?}, got x={real_x_pos:02X} y={real_y_pos:02X} frame={real_frame:02X} animation_delay={real_animation_delay:02X}",
            cpu.pc, ctx.enemy_frame, ctx.animation_delay, ctx.x_pos, ctx.y_pos, expected.outcome
        );
    }
}
