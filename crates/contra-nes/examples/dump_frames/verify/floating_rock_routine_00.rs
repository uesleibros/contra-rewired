/// Captured inputs for one real `floating_rock_routine_00` call
/// (`$97e9` - also `moving_flame_routine_00`, the identical function).
#[derive(Clone, Copy)]
pub struct FloatingRockRoutine00Ctx {
    pub x: usize,
    pub attributes: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub routine: u8,
}

pub fn verify_floating_rock_routine_00(
    ctx: FloatingRockRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::rock::floating_rock_routine_00;

    let x = ctx.x;
    let expected = floating_rock_routine_00(ctx.attributes, ctx.level_scrolling_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos, ctx.routine);
    *checked += 1;

    let real_x_vel_fract = bus.ram[0x518 + x];
    let real_x_vel_fast = bus.ram[0x508 + x];
    let real_var_2 = bus.ram[0x5C8 + x];
    let real_var_1 = bus.ram[0x5B8 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_x_vel_fract != expected.x_vel_fract
        || real_x_vel_fast != expected.x_vel_fast
        || real_var_2 != expected.var_2
        || real_var_1 != expected.var_1
        || real_x_pos != expected.scroll.x_pos
        || real_y_pos != expected.scroll.y_pos
        || real_routine != expected.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(floating_rock_routine_00) frame={frame} pc={:04X} in=(attrs={:02X} x={:02X} y={:02X} routine={:02X}): expected {:?}, got x_vel_fract={real_x_vel_fract:02X} x_vel_fast={real_x_vel_fast:02X} var_2={real_var_2:02X} var_1={real_var_1:02X} x={real_x_pos:02X} y={real_y_pos:02X} routine={real_routine:02X}",
            cpu.pc, ctx.attributes, ctx.x_pos, ctx.y_pos, ctx.routine, expected
        );
    }
}
