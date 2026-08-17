/// Captured inputs for one real `four_soldiers_routine_02` call
/// (`$9582`).
#[derive(Clone, Copy)]
pub struct FourSoldiersRoutine02Ctx {
    pub x: usize,
    pub frame_counter: u8,
    pub enemy_frame: u8,
    pub sprite_attr: u8,
    pub x_vel_accum: u8,
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    pub x_pos: u8,
    pub animation_delay: u8,
    pub soldier_index: u8,
    pub times_fired: u8,
    pub routine: u8,
}

pub fn verify_four_soldiers_routine_02(
    ctx: FourSoldiersRoutine02Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::four_soldiers::{four_soldiers_routine_02, FourSoldiersRoutine02Outcome};
    use contra_native::enemy::indoor_soldier::ApplyEnemyVelocityOutcome;

    let x = ctx.x;
    let expected = four_soldiers_routine_02(
        ctx.frame_counter,
        ctx.enemy_frame,
        ctx.sprite_attr,
        ctx.x_vel_accum,
        ctx.x_vel_fract,
        ctx.x_vel_fast,
        ctx.x_pos,
        ctx.animation_delay,
        ctx.soldier_index,
        ctx.times_fired,
        ctx.routine,
    );
    *checked += 1;

    let real_sprites = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_x_vel_accum = bus.ram[0x4D8 + x];
    let real_animation_delay = bus.ram[0x538 + x];
    let real_var_2 = bus.ram[0x5C8 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let expected_sprite_attr = match expected.velocity.outcome {
        ApplyEnemyVelocityOutcome::Removed(_) => expected.sprite.sprite_attr,
        ApplyEnemyVelocityOutcome::BgPriority(attr) => attr,
    };

    let mut mismatch = real_sprite_attr != expected_sprite_attr
        || real_x_pos != expected.velocity.x_pos
        || real_x_vel_accum != expected.velocity.x_vel_accum;

    match expected.outcome {
        FourSoldiersRoutine02Outcome::StillMoving { animation_delay } => {
            mismatch = mismatch || real_animation_delay != animation_delay;
        }
        FourSoldiersRoutine02Outcome::Fired { sprites, times_fired, animation_delay, routine_update } => {
            mismatch = mismatch
                || real_sprites != sprites
                || real_var_2 != times_fired
                || real_animation_delay != animation_delay
                || real_routine != routine_update.routine;
        }
    }

    if mismatch {
        eprintln!(
            "MISMATCH(four_soldiers_routine_02) frame={frame} pc={:04X} in=(delay={:02X} soldier_index={:02X} times_fired={:02X} x_pos={:02X} routine={:02X}): expected {:?}, got sprites={real_sprites:02X} sprite_attr={real_sprite_attr:02X} x={real_x_pos:02X} x_vel_accum={real_x_vel_accum:02X} animation_delay={real_animation_delay:02X} var_2={real_var_2:02X} routine={real_routine:02X}",
            cpu.pc, ctx.animation_delay, ctx.soldier_index, ctx.times_fired, ctx.x_pos, ctx.routine, expected
        );
    }
}
