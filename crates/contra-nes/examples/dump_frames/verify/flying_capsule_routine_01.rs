/// Captured inputs for one real `flying_capsule_routine_01` call
/// (`$835d`).
#[derive(Clone, Copy)]
pub struct FlyingCapsuleRoutine01Ctx {
    pub x: usize,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub x_vel_accum: u8,
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    pub var_2: u8,
    pub y_pos: u8,
    pub y_vel_accum: u8,
    pub y_vel_fract: u8,
    pub y_vel_fast: u8,
    pub var_1: u8,
}

pub fn verify_flying_capsule_routine_01(
    ctx: FlyingCapsuleRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::flying_capsule::flying_capsule_routine_01;

    let x = ctx.x;
    let expected = flying_capsule_routine_01(
        ctx.level_scrolling_type,
        ctx.frame_scroll,
        ctx.x_pos,
        ctx.x_vel_accum,
        ctx.x_vel_fract,
        ctx.x_vel_fast,
        ctx.var_2,
        ctx.y_pos,
        ctx.y_vel_accum,
        ctx.y_vel_fract,
        ctx.y_vel_fast,
        ctx.var_1,
    );
    *checked += 1;

    let real_sprites = bus.ram[0x30A + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_x_vel_accum = bus.ram[0x4D8 + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_y_vel_accum = bus.ram[0x4C8 + x];

    let mismatch = real_sprites != expected.sprites
        || real_x_pos != expected.position.x.pos
        || real_x_vel_accum != expected.position.x.vel_accum
        || real_y_pos != expected.position.y.pos
        || real_y_vel_accum != expected.position.y.vel_accum;

    if mismatch {
        eprintln!(
            "MISMATCH(flying_capsule_routine_01) frame={frame} pc={:04X} in=(scroll_type={:02X} x={:02X} y={:02X} var_1={:02X} var_2={:02X}): expected {:?}, got sprites={real_sprites:02X} x={real_x_pos:02X} y={real_y_pos:02X}",
            cpu.pc, ctx.level_scrolling_type, ctx.x_pos, ctx.y_pos, ctx.var_1, ctx.var_2, expected
        );
    }
}
