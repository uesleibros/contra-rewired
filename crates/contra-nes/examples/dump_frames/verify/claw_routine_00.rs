/// Captured inputs for one real `claw_routine_00` call (`$aec3`).
#[derive(Clone, Copy)]
pub struct ClawRoutine00Ctx {
    pub x: usize,
    pub attributes: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub routine: u8,
}

pub fn verify_claw_routine_00(
    ctx: ClawRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::claw::claw_routine_00;

    let x = ctx.x;
    let expected = claw_routine_00(ctx.attributes, ctx.level_scrolling_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos, ctx.routine);
    *checked += 1;

    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_frame = bus.ram[0x568 + x];
    let real_attributes = bus.ram[0x5A8 + x];
    let real_animation_delay = bus.ram[0x538 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_x_pos != expected.scroll.x_pos
        || real_y_pos != expected.scroll.y_pos
        || real_frame != expected.frame
        || real_attributes != expected.attributes
        || real_animation_delay != expected.delayed_routine.animation_delay
        || real_routine != expected.delayed_routine.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(claw_routine_00) frame={frame} pc={:04X} in=(attrs={:02X} x={:02X} y={:02X} routine={:02X}): expected {:?}, got x={real_x_pos:02X} y={real_y_pos:02X} frame={real_frame:02X} attrs={real_attributes:02X} animation_delay={real_animation_delay:02X} routine={real_routine:02X}",
            cpu.pc, ctx.attributes, ctx.x_pos, ctx.y_pos, ctx.routine, expected
        );
    }
}
