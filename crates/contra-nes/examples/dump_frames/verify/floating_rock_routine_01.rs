/// Captured inputs for one real `floating_rock_routine_01` call
/// (`$981c`).
#[derive(Clone, Copy)]
pub struct FloatingRockRoutine01Ctx {
    pub x: usize,
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
    pub var_1: u8,
    pub var_2: u8,
}

pub fn verify_floating_rock_routine_01(
    ctx: FloatingRockRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::rock::{floating_rock_routine_01, TurnAroundOutcome};

    let x = ctx.x;
    let expected = floating_rock_routine_01(
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
        ctx.var_1,
        ctx.var_2,
    );
    *checked += 1;

    let real_sprite = bus.ram[0x30A + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_x_vel_accum = bus.ram[0x4D8 + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_y_vel_accum = bus.ram[0x4C8 + x];
    let real_x_vel_fract = bus.ram[0x518 + x];
    let real_x_vel_fast = bus.ram[0x508 + x];

    let base_ok = real_sprite == expected.sprite
        && real_x_pos == expected.inner.position.x.pos
        && real_x_vel_accum == expected.inner.position.x.vel_accum
        && real_y_pos == expected.inner.position.y.pos
        && real_y_vel_accum == expected.inner.position.y.vel_accum;

    let outcome_ok = match expected.inner.outcome {
        TurnAroundOutcome::NoTurn => true,
        TurnAroundOutcome::TurnedAround { x_vel_fract, x_vel_fast } => real_x_vel_fract == x_vel_fract && real_x_vel_fast == x_vel_fast,
    };

    let mismatch = !base_ok || !outcome_ok;

    if mismatch {
        eprintln!(
            "MISMATCH(floating_rock_routine_01) frame={frame} pc={:04X} in=(x={:02X} y={:02X} var_1={:02X} var_2={:02X}): expected {:?}, got sprite={real_sprite:02X} x={real_x_pos:02X} y={real_y_pos:02X} x_vel_fract={real_x_vel_fract:02X} x_vel_fast={real_x_vel_fast:02X}",
            cpu.pc, ctx.x_pos, ctx.y_pos, ctx.var_1, ctx.var_2, expected
        );
    }
}
