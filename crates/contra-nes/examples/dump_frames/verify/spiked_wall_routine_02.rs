/// Captured inputs for one real `spiked_wall_routine_02` call
/// (`$b091`).
#[derive(Clone, Copy)]
pub struct SpikedWallRoutine02Ctx {
    pub x: usize,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub routine: u8,
}

pub fn verify_spiked_wall_routine_02(
    ctx: SpikedWallRoutine02Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::spiked_wall::spiked_wall_routine_02;

    let x = ctx.x;
    let expected = spiked_wall_routine_02(ctx.level_scrolling_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos, ctx.routine);
    *checked += 1;

    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_x_pos != expected.scroll.x_pos || real_y_pos != expected.scroll.y_pos || real_routine != expected.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(spiked_wall_routine_02) frame={frame} pc={:04X} in=(x={:02X} y={:02X} routine={:02X}): expected {:?}, got x={real_x_pos:02X} y={real_y_pos:02X} routine={real_routine:02X}",
            cpu.pc, ctx.x_pos, ctx.y_pos, ctx.routine, expected
        );
    }
}
