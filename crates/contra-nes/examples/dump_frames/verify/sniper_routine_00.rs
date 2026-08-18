/// Captured inputs for one real `sniper_routine_00` call (`$8958`).
#[derive(Clone, Copy)]
pub struct SniperRoutine00Ctx {
    pub x: usize,
    pub sniper_type: u8,
    pub y_pos: u8,
    pub vertical_scroll: u8,
    pub routine: u8,
}

pub fn verify_sniper_routine_00(
    ctx: SniperRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::sniper::sniper_routine_00;

    let x = ctx.x;
    let expected = sniper_routine_00(ctx.sniper_type, ctx.y_pos, ctx.vertical_scroll, ctx.routine);
    *checked += 1;

    let real_animation_delay = bus.ram[0x538 + x];
    let real_frame = bus.ram[0x568 + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_animation_delay != expected.animation_delay
        || real_frame != expected.frame
        || real_y_pos != expected.y_pos
        || real_routine != expected.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(sniper_routine_00) frame={frame} pc={:04X} in=(sniper_type={:02X} y={:02X} vscroll={:02X} routine={:02X}): expected {:?}, got animation_delay={real_animation_delay:02X} frame_num={real_frame:02X} y={real_y_pos:02X} routine={real_routine:02X}",
            cpu.pc, ctx.sniper_type, ctx.y_pos, ctx.vertical_scroll, ctx.routine, expected
        );
    }
}
