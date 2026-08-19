/// Captured inputs for one real `falling_rock_routine_01` call
/// (`$988e`).
#[derive(Clone, Copy)]
pub struct FallingRockRoutine01Ctx {
    pub x: usize,
    pub frame_counter: u8,
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
    pub state_width: u8,
    pub routine: u8,
}

pub fn verify_falling_rock_routine_01(
    ctx: FallingRockRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::rock::{falling_rock_routine_01, FallingRockRoutine01Outcome};

    let x = ctx.x;
    let expected = falling_rock_routine_01(
        ctx.frame_counter,
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
        ctx.state_width,
        ctx.routine,
    );
    *checked += 1;

    let real_sprite = bus.ram[0x30A + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_animation_delay = bus.ram[0x538 + x];
    let real_state_width = bus.ram[0x598 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let base_ok = real_sprite == expected.sprite && real_x_pos == expected.x_pos && real_y_pos == expected.position.y.pos;

    let outcome_ok = match expected.outcome {
        FallingRockRoutine01Outcome::Waiting { animation_delay } => real_animation_delay == animation_delay,
        FallingRockRoutine01Outcome::Activated { state_width, delayed_routine } => {
            real_state_width == state_width && real_animation_delay == delayed_routine.animation_delay && real_routine == delayed_routine.routine_update.routine
        }
    };

    let mismatch = !base_ok || !outcome_ok;

    if mismatch {
        eprintln!(
            "MISMATCH(falling_rock_routine_01) frame={frame} pc={:04X} in=(fc={:02X} x={:02X} y={:02X} delay={:02X} routine={:02X}): expected {:?}, got sprite={real_sprite:02X} x={real_x_pos:02X} y={real_y_pos:02X} animation_delay={real_animation_delay:02X} state_width={real_state_width:02X} routine={real_routine:02X}",
            cpu.pc, ctx.frame_counter, ctx.x_pos, ctx.y_pos, ctx.animation_delay, ctx.routine, expected
        );
    }
}
