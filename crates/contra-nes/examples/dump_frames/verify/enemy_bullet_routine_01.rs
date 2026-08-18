/// Captured inputs for one real `enemy_bullet_routine_01` call
/// (`$8161`).
#[derive(Clone)]
pub struct EnemyBulletRoutine01Ctx {
    pub x: usize,
    pub bullet_type: u8,
    pub current_level: u8,
    pub level_solid_bg_collision_check: u8,
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
    pub vertical_scroll: u8,
    pub horizontal_scroll: u8,
    pub ppuctrl_settings: u8,
    pub bg_collision_data: [u8; contra_native::physics::collision::BG_COLLISION_DATA_LEN],
    pub frame_counter: u8,
    pub routine: u8,
}

pub fn verify_enemy_bullet_routine_01(
    ctx: EnemyBulletRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::enemy_bullet::{enemy_bullet_routine_01, EnemyBulletRoutine01Outcome};
    use contra_native::enemy::update_enemy_pos::RemovedEnemy;

    let x = ctx.x;
    let expected = enemy_bullet_routine_01(
        ctx.bullet_type,
        ctx.current_level,
        ctx.level_solid_bg_collision_check,
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
        ctx.vertical_scroll,
        ctx.horizontal_scroll,
        ctx.ppuctrl_settings,
        &ctx.bg_collision_data,
        ctx.frame_counter,
        ctx.routine,
    );
    *checked += 1;

    let real_sprites = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_x_vel_accum = bus.ram[0x4D8 + x];
    let real_y_vel_accum = bus.ram[0x4C8 + x];
    let real_y_vel_fract = bus.ram[0x4F8 + x];
    let real_y_vel_fast = bus.ram[0x4E8 + x];
    let real_routine = bus.ram[0x4B8 + x];
    let real_frame = bus.ram[0x568 + x];
    let real_animation_delay = bus.ram[0x538 + x];

    let pos_ok = real_x_pos == expected.position.x.pos && real_x_vel_accum == expected.position.x.vel_accum && real_y_pos == expected.position.y.pos && real_y_vel_accum == expected.position.y.vel_accum;

    // `update_enemy_pos`'s own internal removal (a real tail `jmp
    // remove_enemy`) already overwrote ENEMY_SPRITES/ENEMY_ROUTINE to 0
    // by the time this routine's remaining code (including the sprite
    // write already made at the top of this call) is done - so the
    // *baseline* expected sprites/routine must start from that, not
    // from `expected.sprites`/the entry-time routine, before any of the
    // outcome-specific overrides below are applied on top.
    let (baseline_sprites, baseline_routine) = match expected.position.removed {
        Some(r) => (r.sprites, r.routine),
        None => (expected.sprites, ctx.routine),
    };

    // `DragonOrbAnimated` overwrites ENEMY_SPRITE_ATTR a second time
    // (after step 2's own write, which `expected.sprite_attr` reflects);
    // every other outcome leaves step 2's write as the final value.
    let expected_sprite_attr = match expected.outcome {
        EnemyBulletRoutine01Outcome::DragonOrbAnimated { sprite_attr } => sprite_attr,
        _ => expected.sprite_attr,
    };

    let mismatch = !pos_ok
        || real_sprite_attr != expected_sprite_attr
        || match expected.outcome {
            EnemyBulletRoutine01Outcome::RemovedBySolidCollision(RemovedEnemy { routine, sprites }) => real_routine != routine || real_sprites != sprites,
            EnemyBulletRoutine01Outcome::Exited => real_sprites != baseline_sprites || real_routine != baseline_routine,
            EnemyBulletRoutine01Outcome::StillFalling { y_velocity } => {
                real_sprites != baseline_sprites || real_routine != baseline_routine || (real_y_vel_fract, real_y_vel_fast) != y_velocity
            }
            EnemyBulletRoutine01Outcome::Exploded { frame, animation_delay, routine_update } => {
                real_sprites != baseline_sprites
                    || real_frame != frame
                    || real_animation_delay != animation_delay
                    || real_routine != routine_update.routine
                    || routine_update.sprites.map(|s| real_sprites != s).unwrap_or(false)
            }
            EnemyBulletRoutine01Outcome::IndoorRemoved(RemovedEnemy { routine, sprites }) => real_routine != routine || real_sprites != sprites,
            EnemyBulletRoutine01Outcome::IndoorOnScreen => real_sprites != baseline_sprites || real_routine != baseline_routine,
            EnemyBulletRoutine01Outcome::DragonOrbAnimated { .. } => real_sprites != baseline_sprites || real_routine != baseline_routine,
        };

    if mismatch {
        eprintln!(
            "MISMATCH(enemy_bullet_routine_01) frame={frame} pc={:04X} in=(bullet_type={:02X} level={:02X} solid_check={:02X} x={:02X} y={:02X}): expected {:?}, got sprites={real_sprites:02X} sprite_attr={real_sprite_attr:02X} x={real_x_pos:02X} y={real_y_pos:02X} routine={real_routine:02X} frame={real_frame:02X} animation_delay={real_animation_delay:02X}",
            cpu.pc, ctx.bullet_type, ctx.current_level, ctx.level_solid_bg_collision_check, ctx.x_pos, ctx.y_pos, expected
        );
    }
}
