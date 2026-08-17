/// Captured inputs for one real `shared_enemy_routine_01` call (`$9360`)
/// - a real, shared enemy-routine-table entry (indoor soldier family's
/// own routine index 3).
#[derive(Clone, Copy)]
pub struct SharedEnemyRoutine01Ctx {
    pub x: usize,
    pub scroll_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub x_vel_accum: u8,
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    pub y_pos: u8,
    pub y_vel_accum: u8,
    pub y_vel_fract: u8,
    pub y_vel_fast: u8,
    pub animation_delay: u8,
    pub routine: u8,
}

pub fn verify_shared_enemy_routine_01(
    ctx: SharedEnemyRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::indoor_soldier::{shared_enemy_routine_01, SharedEnemyRoutine01Outcome};

    let x = ctx.x;
    let expected = shared_enemy_routine_01(
        ctx.scroll_type,
        ctx.frame_scroll,
        ctx.x_pos,
        ctx.x_vel_accum,
        ctx.x_vel_fract,
        ctx.x_vel_fast,
        ctx.y_pos,
        ctx.y_vel_accum,
        ctx.y_vel_fract,
        ctx.y_vel_fast,
        ctx.animation_delay,
        ctx.routine,
    );
    *checked += 1;

    let real_x_pos = bus.ram[0x33E + x];
    let real_x_vel_accum = bus.ram[0x4D8 + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_y_vel_accum = bus.ram[0x4C8 + x];
    let real_y_vel_fract = bus.ram[0x4F8 + x];
    let real_y_vel_fast = bus.ram[0x4E8 + x];
    let real_animation_delay = bus.ram[0x538 + x];
    let real_routine = bus.ram[0x4B8 + x];
    let real_sprites = bus.ram[0x30A + x];

    let pos_ok = real_x_pos == expected.position.x.pos
        && real_x_vel_accum == expected.position.x.vel_accum
        && real_y_pos == expected.position.y.pos
        && real_y_vel_accum == expected.position.y.vel_accum
        && real_y_vel_fract == expected.y_velocity.0
        && real_y_vel_fast == expected.y_velocity.1;

    let mismatch = !pos_ok
        || match expected.outcome {
            SharedEnemyRoutine01Outcome::Waiting => real_animation_delay != expected.animation_delay,
            SharedEnemyRoutine01Outcome::Advanced(routine_update) => {
                real_routine != routine_update.routine || routine_update.sprites.map(|s| real_sprites != s).unwrap_or(false)
            }
        };

    if mismatch {
        eprintln!(
            "MISMATCH(shared_enemy_routine_01) frame={frame} pc={:04X} in=(scroll_type={:02X} frame_scroll={:02X} x={:02X} x_vel=({:02X},{:02X},{:02X}) y={:02X} y_vel=({:02X},{:02X},{:02X}) animation_delay={:02X} routine={:02X}): expected {:?}, got x={real_x_pos:02X} x_vel_accum={real_x_vel_accum:02X} y={real_y_pos:02X} y_vel_accum={real_y_vel_accum:02X} y_vel=({real_y_vel_fract:02X},{real_y_vel_fast:02X}) animation_delay={real_animation_delay:02X} routine={real_routine:02X} sprites={real_sprites:02X}",
            cpu.pc,
            ctx.scroll_type,
            ctx.frame_scroll,
            ctx.x_pos,
            ctx.x_vel_accum,
            ctx.x_vel_fract,
            ctx.x_vel_fast,
            ctx.y_pos,
            ctx.y_vel_accum,
            ctx.y_vel_fract,
            ctx.y_vel_fast,
            ctx.animation_delay,
            ctx.routine,
            expected
        );
    }
}
