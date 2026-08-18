use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

/// Captured inputs for one real `turret_man_routine_02` call (`$f0ec`).
#[derive(Clone, Copy)]
pub struct TurretManRoutine02Ctx {
    pub x: usize,
    pub current_level: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub enemy_attributes: u8,
    pub enemy_sprites: u8,
    pub animation_delay: u8,
    pub routine: u8,
    pub enemy_routine_slots: [u8; ENEMY_SLOT_COUNT],
}

pub fn verify_turret_man_routine_02(
    ctx: TurretManRoutine02Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::turret_man::{turret_man_routine_02, TurretManRoutine02Outcome};

    let x = ctx.x;
    let expected = turret_man_routine_02(
        prg_rom,
        &ctx.enemy_routine_slots,
        ctx.current_level,
        ctx.level_scrolling_type,
        ctx.frame_scroll,
        ctx.x_pos,
        ctx.y_pos,
        ctx.enemy_attributes,
        ctx.enemy_sprites,
        ctx.animation_delay,
        ctx.routine,
    );
    *checked += 1;

    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_animation_delay = bus.ram[0x538 + x];
    let real_sprite = bus.ram[0x30A + x];
    let real_routine = bus.ram[0x4B8 + x];

    let pos_ok = real_x_pos == expected.scroll.x_pos && real_y_pos == expected.scroll.y_pos;

    let mismatch = !pos_ok
        || match &expected.outcome {
            TurretManRoutine02Outcome::Waiting { animation_delay } => real_animation_delay != *animation_delay,
            TurretManRoutine02Outcome::Fired { bullet, animation_delay, sprite, routine_update, .. } => {
                let bullet_ok = match bullet {
                    Some(b) => {
                        let bx = b.slot as usize;
                        bus.ram[0x33E + bx] == b.x_pos && bus.ram[0x324 + bx] == b.y_pos && bus.ram[0x528 + bx] == 0x0F && bus.ram[0x4B8 + bx] == b.initialized.routine
                    }
                    None => true,
                };
                !bullet_ok || real_animation_delay != *animation_delay || real_sprite != *sprite || real_routine != routine_update.routine
            }
        };

    if mismatch {
        eprintln!(
            "MISMATCH(turret_man_routine_02) frame={frame} pc={:04X} in=(x={:02X} y={:02X} attrs={:02X} sprite={:02X} delay={:02X} routine={:02X}): expected {:?}, got x={real_x_pos:02X} y={real_y_pos:02X} animation_delay={real_animation_delay:02X} sprite={real_sprite:02X} routine={real_routine:02X}",
            cpu.pc, ctx.x_pos, ctx.y_pos, ctx.enemy_attributes, ctx.enemy_sprites, ctx.animation_delay, ctx.routine, expected.outcome
        );
    }
}
