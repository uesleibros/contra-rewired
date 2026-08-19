use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

/// Captured inputs for one real `ice_grenade_generator_routine_01` call
/// (`$a399`).
#[derive(Clone, Copy)]
pub struct IceGrenadeGeneratorRoutine01Ctx {
    pub x: usize,
    pub current_level: u8,
    pub animation_delay: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub enemy_routine_slots: [u8; ENEMY_SLOT_COUNT],
}

pub fn verify_ice_grenade_generator_routine_01(
    ctx: IceGrenadeGeneratorRoutine01Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::ice::{ice_grenade_generator_routine_01, IceGrenadeGeneratorRoutine01Outcome};

    let x = ctx.x;
    let expected = ice_grenade_generator_routine_01(prg_rom, &ctx.enemy_routine_slots, ctx.current_level, ctx.animation_delay, ctx.level_scrolling_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos);
    *checked += 1;

    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_animation_delay = bus.ram[0x538 + x];

    let pos_ok = real_x_pos == expected.scroll.x_pos && real_y_pos == expected.scroll.y_pos;

    let outcome_ok = match &expected.outcome {
        IceGrenadeGeneratorRoutine01Outcome::Waiting { animation_delay } => real_animation_delay == *animation_delay,
        IceGrenadeGeneratorRoutine01Outcome::Spawned { animation_delay, grenade } => {
            let grenade_ok = match grenade {
                Some(g) => {
                    let gx = g.slot as usize;
                    bus.ram[0x33E + gx] == g.x_pos && bus.ram[0x324 + gx] == g.y_pos && bus.ram[0x528 + gx] == 0x11 && bus.ram[0x4B8 + gx] == g.initialized.routine
                }
                None => true,
            };
            grenade_ok && real_animation_delay == *animation_delay
        }
    };

    let mismatch = !pos_ok || !outcome_ok;

    if mismatch {
        eprintln!(
            "MISMATCH(ice_grenade_generator_routine_01) frame={frame} pc={:04X} in=(delay={:02X} x={:02X} y={:02X}): expected {:?}, got x={real_x_pos:02X} y={real_y_pos:02X} animation_delay={real_animation_delay:02X}",
            cpu.pc, ctx.animation_delay, ctx.x_pos, ctx.y_pos, expected.outcome
        );
    }
}
