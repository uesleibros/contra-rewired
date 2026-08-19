/// Captured inputs for one real `rising_spiked_wall_routine_03` call
/// (`$b200`).
#[derive(Clone, Copy)]
pub struct RisingSpikedWallRoutine03Ctx {
    pub x: usize,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
}

pub fn verify_rising_spiked_wall_routine_03(
    ctx: RisingSpikedWallRoutine03Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::spiked_wall::rising_spiked_wall_routine_03;

    let x = ctx.x;
    let expected = rising_spiked_wall_routine_03(ctx.level_scrolling_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos);
    *checked += 1;

    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];

    let mismatch = real_x_pos != expected.x_pos || real_y_pos != expected.y_pos;

    if mismatch {
        eprintln!(
            "MISMATCH(rising_spiked_wall_routine_03) frame={frame} pc={:04X} in=(x={:02X} y={:02X}): expected {:?}, got x={real_x_pos:02X} y={real_y_pos:02X}",
            cpu.pc, ctx.x_pos, ctx.y_pos, expected
        );
    }
}
