/// Captured inputs for one real `falling_rock_routine_02` call
/// (`$98ce`).
#[derive(Clone, Copy)]
pub struct FallingRockRoutine02Ctx {
    pub x: usize,
    pub frame_counter: u8,
    pub sprite_attr: u8,
    pub y_pos: u8,
    pub var_1: u8,
    pub vertical_scroll: u8,
    pub horizontal_scroll: u8,
    pub ppuctrl_settings: u8,
    pub bg_collision_data: [u8; contra_native::physics::collision::BG_COLLISION_DATA_LEN],
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub x_vel_accum: u8,
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    pub y_vel_accum: u8,
    pub y_vel_fract: u8,
    pub y_vel_fast: u8,
}

pub fn verify_falling_rock_routine_02(
    ctx: FallingRockRoutine02Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::rock::falling_rock_routine_02;

    let x = ctx.x;
    let expected = falling_rock_routine_02(
        ctx.frame_counter,
        ctx.sprite_attr,
        ctx.y_pos,
        ctx.var_1,
        ctx.vertical_scroll,
        ctx.horizontal_scroll,
        ctx.ppuctrl_settings,
        &ctx.bg_collision_data,
        ctx.level_scrolling_type,
        ctx.frame_scroll,
        ctx.x_pos,
        ctx.x_vel_accum,
        ctx.x_vel_fract,
        ctx.x_vel_fast,
        ctx.y_vel_accum,
        ctx.y_vel_fract,
        ctx.y_vel_fast,
    );
    *checked += 1;

    let real_sprite = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_var_1 = bus.ram[0x5B8 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_x_vel_accum = bus.ram[0x4D8 + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_y_vel_accum = bus.ram[0x4C8 + x];

    let mismatch = real_sprite != expected.sprite
        || real_sprite_attr != expected.sprite_attr
        || real_var_1 != expected.var_1
        || real_x_pos != expected.position.x.pos
        || real_x_vel_accum != expected.position.x.vel_accum
        || real_y_pos != expected.position.y.pos
        || real_y_vel_accum != expected.position.y.vel_accum;

    if mismatch {
        eprintln!(
            "MISMATCH(falling_rock_routine_02) frame={frame} pc={:04X} in=(fc={:02X} y={:02X} var_1={:02X} x={:02X}): expected {:?}, got sprite={real_sprite:02X} sprite_attr={real_sprite_attr:02X} var_1={real_var_1:02X} x={real_x_pos:02X} y={real_y_pos:02X}",
            cpu.pc, ctx.frame_counter, ctx.y_pos, ctx.var_1, ctx.x_pos, expected
        );
    }
}
