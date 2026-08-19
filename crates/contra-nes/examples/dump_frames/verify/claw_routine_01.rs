/// Captured inputs for one real `claw_routine_01` call (`$aee5`).
#[derive(Clone, Copy)]
pub struct ClawRoutine01Ctx {
    pub x: usize,
    pub attributes: u8,
    pub animation_delay: u8,
    pub frame_counter: u8,
    pub enemy_frame: u8,
    pub sprite_x_pos: [u8; 2],
    pub player_state: [u8; 2],
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub routine: u8,
}

pub fn verify_claw_routine_01(
    ctx: ClawRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::claw::{claw_routine_01, ClawRoutine01Outcome};

    let x = ctx.x;
    let expected = claw_routine_01(
        ctx.attributes,
        ctx.animation_delay,
        ctx.frame_counter,
        ctx.enemy_frame,
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
    let real_animation_delay = bus.ram[0x538 + x];
    let real_var_2 = bus.ram[0x5C8 + x];
    let real_var_3 = bus.ram[0x5D8 + x];
    let real_var_4 = bus.ram[0x5E8 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let pos_ok = real_x_pos == expected.scroll.x_pos && real_y_pos == expected.scroll.y_pos;

    let outcome_ok = match expected.outcome {
        ClawRoutine01Outcome::Waiting => true,
        ClawRoutine01Outcome::SeekingDelayCountdown { animation_delay } => real_animation_delay == animation_delay,
        ClawRoutine01Outcome::Descending { var_2, var_3, var_4, delayed_routine } => {
            real_var_2 == var_2 && real_var_3 == var_3 && real_var_4 == var_4 && real_animation_delay == delayed_routine.animation_delay && real_routine == delayed_routine.routine_update.routine
        }
    };

    let mismatch = !pos_ok || !outcome_ok;

    if mismatch {
        eprintln!(
            "MISMATCH(claw_routine_01) frame={frame} pc={:04X} in=(attrs={:02X} delay={:02X} fc={:02X} ef={:02X} x={:02X} y={:02X} routine={:02X}): expected {:?}, got x={real_x_pos:02X} y={real_y_pos:02X} animation_delay={real_animation_delay:02X} var_2={real_var_2:02X} var_3={real_var_3:02X} var_4={real_var_4:02X} routine={real_routine:02X}",
            cpu.pc, ctx.attributes, ctx.animation_delay, ctx.frame_counter, ctx.enemy_frame, ctx.x_pos, ctx.y_pos, ctx.routine, expected
        );
    }
}
