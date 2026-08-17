/// Captured inputs for one real `shared_enemy_routine_00` call (`$9346`)
/// - a real, shared enemy-routine-table entry (indoor soldier family's
/// own routine index 2).
#[derive(Clone, Copy)]
pub struct SharedEnemyRoutine00Ctx {
    pub x: usize,
    pub state_width: u8,
    pub routine: u8,
}

pub fn verify_shared_enemy_routine_00(
    ctx: SharedEnemyRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::indoor_soldier::shared_enemy_routine_00;

    let x = ctx.x;
    let expected = shared_enemy_routine_00(ctx.state_width, ctx.routine);
    *checked += 1;

    let real_state_width = bus.ram[0x598 + x];
    let real_sprites = bus.ram[0x30A + x];
    let real_y_vel_fract = bus.ram[0x4F8 + x];
    let real_y_vel_fast = bus.ram[0x4E8 + x];
    let real_x_vel_fract = bus.ram[0x518 + x];
    let real_x_vel_fast = bus.ram[0x508 + x];
    let real_animation_delay = bus.ram[0x538 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_state_width != expected.state_width
        || real_sprites != expected.sprites
        || real_y_vel_fract != expected.y_velocity_fract
        || real_y_vel_fast != expected.y_velocity_fast
        || real_x_vel_fract != expected.x_velocity.vel_fract
        || real_x_vel_fast != expected.x_velocity.vel_fast
        || real_animation_delay != expected.delayed_routine.animation_delay
        || real_routine != expected.delayed_routine.routine_update.routine
        || expected.delayed_routine.routine_update.sprites.map(|s| real_sprites != s).unwrap_or(false);

    if mismatch {
        eprintln!(
            "MISMATCH(shared_enemy_routine_00) frame={frame} pc={:04X} in=(state_width={:02X} routine={:02X}): expected {:?}, got state_width={real_state_width:02X} sprites={real_sprites:02X} y_vel=({real_y_vel_fract:02X},{real_y_vel_fast:02X}) x_vel=({real_x_vel_fract:02X},{real_x_vel_fast:02X}) animation_delay={real_animation_delay:02X} routine={real_routine:02X}",
            cpu.pc, ctx.state_width, ctx.routine, expected
        );
    }
}
