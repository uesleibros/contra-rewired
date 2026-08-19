/// Captured inputs for one real `mini_ufo_routine_03` call (`$a94c`).
#[derive(Clone, Copy)]
pub struct MiniUfoRoutine03Ctx {
    pub x: usize,
    pub animation_delay: u8,
    pub sprite: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub x_vel_accum: u8,
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    pub y_pos: u8,
    pub y_vel_accum: u8,
    pub y_vel_fract: u8,
    pub y_vel_fast: u8,
}

pub fn verify_mini_ufo_routine_03(
    ctx: MiniUfoRoutine03Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::ufo::mini_ufo_routine_03;

    let x = ctx.x;
    let expected = mini_ufo_routine_03(
        ctx.animation_delay,
        ctx.sprite,
        ctx.level_scrolling_type,
        ctx.frame_scroll,
        ctx.x_pos,
        ctx.x_vel_accum,
        ctx.x_vel_fract,
        ctx.x_vel_fast,
        ctx.y_pos,
        ctx.y_vel_accum,
        ctx.y_vel_fract,
        ctx.y_vel_fast,
    );
    *checked += 1;

    let real_animation_delay = bus.ram[0x538 + x];
    let real_sprite = bus.ram[0x30A + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_x_vel_accum = bus.ram[0x4D8 + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_y_vel_accum = bus.ram[0x4C8 + x];

    let mismatch = real_animation_delay != expected.animation_delay
        || (match expected.sprite {
            Some(s) => real_sprite != s,
            None => real_sprite != ctx.sprite,
        })
        || real_x_pos != expected.position.x.pos
        || real_x_vel_accum != expected.position.x.vel_accum
        || real_y_pos != expected.position.y.pos
        || real_y_vel_accum != expected.position.y.vel_accum;

    if mismatch {
        eprintln!(
            "MISMATCH(mini_ufo_routine_03) frame={frame} pc={:04X} in=(delay={:02X} x={:02X} y={:02X}): expected {:?}, got animation_delay={real_animation_delay:02X} sprite={real_sprite:02X} x={real_x_pos:02X} y={real_y_pos:02X}",
            cpu.pc, ctx.animation_delay, ctx.x_pos, ctx.y_pos, expected
        );
    }
}
