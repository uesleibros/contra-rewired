/// Captured inputs for one real `rising_spiked_wall_routine_01` call
/// (`$b00c`).
#[derive(Clone, Copy)]
pub struct RisingSpikedWallRoutine01Ctx {
    pub x: usize,
    pub var_3: u8,
    pub var_4: u8,
    pub state_width: u8,
    pub sprite_x_pos: [u8; 2],
    pub player_state: [u8; 2],
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub routine: u8,
}

pub fn verify_rising_spiked_wall_routine_01(
    ctx: RisingSpikedWallRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::spiked_wall::{rising_spiked_wall_routine_01, RisingSpikedWallRoutine01Outcome};

    let x = ctx.x;
    let expected = rising_spiked_wall_routine_01(
        ctx.var_3,
        ctx.var_4,
        ctx.state_width,
        ctx.sprite_x_pos,
        ctx.player_state,
        ctx.level_scrolling_type,
        ctx.frame_scroll,
        ctx.x_pos,
        ctx.y_pos,
        ctx.routine,
    );
    *checked += 1;

    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_var_2 = bus.ram[0x5C8 + x];
    let real_state_width = bus.ram[0x598 + x];
    let real_animation_delay = bus.ram[0x538 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let pos_ok = real_x_pos == expected.scroll.x_pos && real_y_pos == expected.scroll.y_pos;

    let outcome_ok = match expected.outcome {
        RisingSpikedWallRoutine01Outcome::Waiting => true,
        RisingSpikedWallRoutine01Outcome::Triggered { var_2, state_width, delayed_routine } => {
            real_var_2 == var_2 && real_state_width == state_width && real_animation_delay == delayed_routine.animation_delay && real_routine == delayed_routine.routine_update.routine
        }
    };

    let mismatch = !pos_ok || !outcome_ok;

    if mismatch {
        eprintln!(
            "MISMATCH(rising_spiked_wall_routine_01) frame={frame} pc={:04X} in=(var_3={:02X} var_4={:02X} x={:02X} y={:02X} routine={:02X}): expected {:?}, got x={real_x_pos:02X} y={real_y_pos:02X} var_2={real_var_2:02X} state_width={real_state_width:02X} animation_delay={real_animation_delay:02X} routine={real_routine:02X}",
            cpu.pc, ctx.var_3, ctx.var_4, ctx.x_pos, ctx.y_pos, ctx.routine, expected
        );
    }
}
