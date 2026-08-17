/// Captured inputs for one real `enemy_routine_init_explosion` call
/// (`$e74b`) - a real, shared enemy-routine-table entry.
#[derive(Clone, Copy)]
pub struct EnemyRoutineInitExplosionCtx {
    pub x: usize,
    pub state_width: u8,
    pub sprite_attr: u8,
    pub sprites: u8,
    pub scroll_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub routine: u8,
}

pub fn verify_enemy_routine_init_explosion(
    ctx: EnemyRoutineInitExplosionCtx,
    sound_seen: Option<u8>,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::enemy_explosion::{enemy_routine_init_explosion, EnemyRoutineInitExplosionOutcome};

    let x = ctx.x;
    let expected =
        enemy_routine_init_explosion(ctx.state_width, ctx.sprite_attr, ctx.sprites, ctx.scroll_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos, ctx.routine);
    *checked += 1;

    let real_state_width = bus.ram[0x598 + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_frame = bus.ram[0x568 + x];
    let real_sprites = bus.ram[0x30A + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_delay = bus.ram[0x538 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let sound_ok = sound_seen == expected.sound;

    let mismatch = !sound_ok
        || real_state_width != expected.state_width
        || real_sprite_attr != expected.sprite_attr
        || match expected.outcome {
            EnemyRoutineInitExplosionOutcome::Removed(_) => real_sprites != 0 || real_routine != 0,
            EnemyRoutineInitExplosionOutcome::Hidden(h) => {
                let removed = h.scroll.should_remove;
                let expected_sprites = if removed { 0 } else { h.enemy_sprites };
                let expected_routine = if removed { 0 } else { h.delayed_routine.routine_update.routine };
                real_frame != h.enemy_frame
                    || real_sprites != expected_sprites
                    || real_x_pos != h.scroll.x_pos
                    || real_y_pos != h.scroll.y_pos
                    || real_delay != h.delayed_routine.animation_delay
                    || real_routine != expected_routine
            }
        };

    if mismatch {
        eprintln!(
            "MISMATCH(enemy_routine_init_explosion) frame={frame} pc={:04X} in=(state_width={:02X} sprite_attr={:02X} sprites={:02X} scroll_type={:02X} frame_scroll={:02X} x={:02X} y={:02X} routine={:02X}): expected {:?} sound_seen={sound_seen:?}, got state_width={real_state_width:02X} sprite_attr={real_sprite_attr:02X} frame={real_frame:02X} sprites={real_sprites:02X} x={real_x_pos:02X} y={real_y_pos:02X} delay={real_delay:02X} routine={real_routine:02X}",
            cpu.pc, ctx.state_width, ctx.sprite_attr, ctx.sprites, ctx.scroll_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos, ctx.routine, expected
        );
    }
}

