/// Captured inputs for one real `rising_spiked_wall_routine_00` call
/// (`$afd6`).
#[derive(Clone, Copy)]
pub struct RisingSpikedWallRoutine00Ctx {
    pub x: usize,
    pub attributes: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub routine: u8,
}

pub fn verify_rising_spiked_wall_routine_00(
    ctx: RisingSpikedWallRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::spiked_wall::rising_spiked_wall_routine_00;

    let x = ctx.x;
    let expected = rising_spiked_wall_routine_00(ctx.attributes, ctx.level_scrolling_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos, ctx.routine);
    *checked += 1;

    let real_var_3 = bus.ram[0x5D8 + x];
    let real_var_4 = bus.ram[0x5E8 + x];
    let real_attack_delay = bus.ram[0x558 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_attributes = bus.ram[0x5A8 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_var_3 != expected.var_3
        || real_var_4 != expected.var_4
        || real_attack_delay != expected.attack_delay
        || real_x_pos != expected.tail.scroll.x_pos
        || real_y_pos != expected.tail.scroll.y_pos
        || real_attributes != expected.tail.attributes
        || real_routine != expected.tail.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(rising_spiked_wall_routine_00) frame={frame} pc={:04X} in=(attrs={:02X} x={:02X} y={:02X} routine={:02X}): expected {:?}, got var_3={real_var_3:02X} var_4={real_var_4:02X} attack_delay={real_attack_delay:02X} x={real_x_pos:02X} y={real_y_pos:02X} attributes={real_attributes:02X} routine={real_routine:02X}",
            cpu.pc, ctx.attributes, ctx.x_pos, ctx.y_pos, ctx.routine, expected
        );
    }
}
