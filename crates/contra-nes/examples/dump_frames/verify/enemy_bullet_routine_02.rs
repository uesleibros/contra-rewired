/// Captured inputs for one real `enemy_bullet_routine_02` call
/// (`$81e4`).
#[derive(Clone, Copy)]
pub struct EnemyBulletRoutine02Ctx {
    pub x: usize,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub animation_delay: u8,
    pub frame: u8,
    pub routine: u8,
}

pub fn verify_enemy_bullet_routine_02(
    ctx: EnemyBulletRoutine02Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::enemy_bullet::{enemy_bullet_routine_02, EnemyBulletRoutine02Outcome};

    let x = ctx.x;
    let expected = enemy_bullet_routine_02(ctx.level_scrolling_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos, ctx.animation_delay, ctx.frame, ctx.routine);
    *checked += 1;

    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_animation_delay = bus.ram[0x538 + x];
    let real_frame = bus.ram[0x568 + x];
    let real_sprites = bus.ram[0x30A + x];
    let real_routine = bus.ram[0x4B8 + x];

    let pos_ok = real_x_pos == expected.scroll.x_pos && real_y_pos == expected.scroll.y_pos;

    let mismatch = !pos_ok
        || match expected.outcome {
            EnemyBulletRoutine02Outcome::Waiting { animation_delay } => real_animation_delay != animation_delay,
            EnemyBulletRoutine02Outcome::Animating { frame, animation_delay, sprites } => {
                real_frame != frame || real_animation_delay != animation_delay || real_sprites != sprites
            }
            EnemyBulletRoutine02Outcome::Advanced(routine_update) => {
                real_routine != routine_update.routine || routine_update.sprites.map(|s| real_sprites != s).unwrap_or(false)
            }
        };

    if mismatch {
        eprintln!(
            "MISMATCH(enemy_bullet_routine_02) frame={frame} pc={:04X} in=(scroll_type={:02X} frame_scroll={:02X} x={:02X} y={:02X} delay={:02X} frame_num={:02X} routine={:02X}): expected {:?}, got x={real_x_pos:02X} y={real_y_pos:02X} animation_delay={real_animation_delay:02X} frame={real_frame:02X} sprites={real_sprites:02X} routine={real_routine:02X}",
            cpu.pc, ctx.level_scrolling_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos, ctx.animation_delay, ctx.frame, ctx.routine, expected
        );
    }
}
