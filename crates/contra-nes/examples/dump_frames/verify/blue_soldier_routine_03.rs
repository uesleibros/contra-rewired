/// Captured inputs for one real `blue_soldier_routine_03` call (`$a245`).
#[derive(Clone, Copy)]
pub struct BlueSoldierRoutine03Ctx {
    pub x: usize,
    pub animation_delay: u8,
    pub scroll_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub x_accum: u8,
    pub x_fract: u8,
    pub x_fast: u8,
    pub y_pos: u8,
    pub y_accum: u8,
    pub y_fract: u8,
    pub y_fast: u8,
}

pub fn verify_blue_soldier_routine_03(
    ctx: BlueSoldierRoutine03Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::red_blue_soldier::blue_soldier_routine_03;

    let x = ctx.x;
    let expected = blue_soldier_routine_03(
        ctx.animation_delay, ctx.scroll_type, ctx.frame_scroll, ctx.x_pos, ctx.x_accum, ctx.x_fract, ctx.x_fast,
        ctx.y_pos, ctx.y_accum, ctx.y_fract, ctx.y_fast,
    );
    *checked += 1;

    let real_sprites = bus.ram[0x30A + x];
    let real_delay = bus.ram[0x538 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let removed = expected.position.removed.is_some();
    let expected_sprites = if removed { 0 } else { expected.sprites };

    let mismatch = real_sprites != expected_sprites
        || real_delay != expected.animation_delay
        || real_x_pos != expected.position.x.pos
        || real_y_pos != expected.position.y.pos
        || (removed && real_routine != 0);

    if mismatch {
        eprintln!(
            "MISMATCH(blue_soldier_routine_03) frame={frame} pc={:04X} in=(delay={:02X} x={:02X} y={:02X}): expected {:?}, got sprites={real_sprites:02X} delay={real_delay:02X} x={real_x_pos:02X} y={real_y_pos:02X} routine={real_routine:02X}",
            cpu.pc, ctx.animation_delay, ctx.x_pos, ctx.y_pos, expected
        );
    }
}
