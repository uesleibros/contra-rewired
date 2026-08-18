/// Captured inputs for one real `roller_routine_01` call (`$8f94`).
#[derive(Clone, Copy)]
pub struct RollerRoutine01Ctx {
    pub x: usize,
    pub y_pos: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub x_vel_accum: u8,
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    pub y_vel_accum: u8,
    pub y_vel_fract: u8,
    pub y_vel_fast: u8,
    pub state_width: u8,
}

pub fn verify_roller_routine_01(
    ctx: RollerRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::roller::{roller_routine_01, RollerRoutine01Outcome};

    let x = ctx.x;
    let expected = roller_routine_01(
        ctx.y_pos,
        ctx.level_scrolling_type,
        ctx.frame_scroll,
        ctx.x_pos,
        ctx.x_vel_accum,
        ctx.x_vel_fract,
        ctx.x_vel_fast,
        ctx.y_vel_accum,
        ctx.y_vel_fract,
        ctx.y_vel_fast,
        ctx.state_width,
    );
    *checked += 1;

    let real_sprite = bus.ram[0x30A + x];
    let real_score_collision = bus.ram[0x588 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_x_vel_accum = bus.ram[0x4D8 + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_y_vel_accum = bus.ram[0x4C8 + x];
    let real_state_width = bus.ram[0x598 + x];

    let base_ok = real_sprite == expected.sprite
        && real_x_pos == expected.position.x.pos
        && real_x_vel_accum == expected.position.x.vel_accum
        && real_y_pos == expected.position.y.pos
        && real_y_vel_accum == expected.position.y.vel_accum
        && match expected.score_collision {
            Some(sc) => real_score_collision == sc,
            None => true,
        };

    let mismatch = !base_ok
        || match expected.outcome {
            RollerRoutine01Outcome::NotCloseEnough => false,
            RollerRoutine01Outcome::CollisionEnabled { state_width } => real_state_width != state_width,
            RollerRoutine01Outcome::Removed(_) => false,
        };

    if mismatch {
        eprintln!(
            "MISMATCH(roller_routine_01) frame={frame} pc={:04X} in=(y={:02X} x={:02X} state_width={:02X}): expected {:?}, got sprite={real_sprite:02X} score_collision={real_score_collision:02X} x={real_x_pos:02X} y={real_y_pos:02X} state_width={real_state_width:02X}",
            cpu.pc, ctx.y_pos, ctx.x_pos, ctx.state_width, expected
        );
    }
}
