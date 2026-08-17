/// Captured inputs for one real `enemy_routine_explosion` call (`$e7b0`)
/// - a real, shared enemy-routine-table entry.
#[derive(Clone, Copy)]
pub struct EnemyRoutineExplosionCtx {
    pub x: usize,
    pub state_width: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub scroll_type: u8,
    pub frame_scroll: u8,
    pub routine: u8,
    pub animation_delay: u8,
    pub frame: u8,
}

pub fn verify_enemy_routine_explosion(
    ctx: EnemyRoutineExplosionCtx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::enemy_explosion::{enemy_routine_explosion, ShowExplosionAOutcome};

    let x = ctx.x;
    let expected = enemy_routine_explosion(
        ctx.state_width, ctx.x_pos, ctx.y_pos, ctx.scroll_type, ctx.frame_scroll, ctx.routine, ctx.animation_delay, ctx.frame,
    );
    *checked += 1;

    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_delay = bus.ram[0x538 + x];
    let real_frame = bus.ram[0x568 + x];
    let real_state_width = bus.ram[0x598 + x];
    let real_sprites = bus.ram[0x30A + x];
    let real_routine = bus.ram[0x4B8 + x];

    let pos_ok = real_x_pos == expected.scroll.x_pos && real_y_pos == expected.scroll.y_pos;

    let mismatch = !pos_ok
        || match expected.outcome {
            ShowExplosionAOutcome::NoRoutine => false,
            ShowExplosionAOutcome::Waiting { animation_delay } => real_delay != animation_delay,
            ShowExplosionAOutcome::Animating { enemy_frame, animation_delay, state_width, enemy_sprites } => {
                real_frame != enemy_frame
                    || real_delay != animation_delay
                    || real_sprites != enemy_sprites
                    || state_width.map(|sw| real_state_width != sw).unwrap_or(false)
            }
            ShowExplosionAOutcome::Advanced(routine_update) => {
                real_routine != routine_update.routine
                    || routine_update.sprites.map(|s| real_sprites != s).unwrap_or(false)
            }
        };

    if mismatch {
        eprintln!(
            "MISMATCH(enemy_routine_explosion) frame={frame} pc={:04X} in=(state_width={:02X} x={:02X} y={:02X} scroll_type={:02X} frame_scroll={:02X} routine={:02X} delay={:02X} frame_num={:02X}): expected {:?}, got x={real_x_pos:02X} y={real_y_pos:02X} delay={real_delay:02X} frame={real_frame:02X} state_width={real_state_width:02X} sprites={real_sprites:02X} routine={real_routine:02X}",
            cpu.pc, ctx.state_width, ctx.x_pos, ctx.y_pos, ctx.scroll_type, ctx.frame_scroll, ctx.routine, ctx.animation_delay, ctx.frame, expected
        );
    }
}

