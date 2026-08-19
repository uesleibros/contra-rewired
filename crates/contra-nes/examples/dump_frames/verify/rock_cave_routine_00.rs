/// Captured inputs for one real `rock_cave_routine_00` call (`$985d`).
#[derive(Clone, Copy)]
pub struct RockCaveRoutine00Ctx {
    pub x: usize,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub routine: u8,
}

pub fn verify_rock_cave_routine_00(
    ctx: RockCaveRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::rock::rock_cave_routine_00;

    let x = ctx.x;
    let (scroll, routine_update) = rock_cave_routine_00(ctx.level_scrolling_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos, ctx.routine);
    *checked += 1;

    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_x_pos != scroll.x_pos || real_y_pos != scroll.y_pos || real_routine != routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(rock_cave_routine_00) frame={frame} pc={:04X} in=(x={:02X} y={:02X} routine={:02X}): expected scroll={scroll:?} routine_update={routine_update:?}, got x={real_x_pos:02X} y={real_y_pos:02X} routine={real_routine:02X}",
            cpu.pc, ctx.x_pos, ctx.y_pos, ctx.routine
        );
    }
}
