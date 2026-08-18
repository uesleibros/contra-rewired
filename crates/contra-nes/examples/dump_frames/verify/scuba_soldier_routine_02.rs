use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

/// Captured inputs for one real `scuba_soldier_routine_02` call
/// (`$f183`).
#[derive(Clone, Copy)]
pub struct ScubaSoldierRoutine02Ctx {
    pub x: usize,
    pub current_level: u8,
    pub var_1: u8,
    pub animation_delay: u8,
    pub attack_delay: u8,
    pub state_width: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub routine: u8,
    pub enemy_routine_slots: [u8; ENEMY_SLOT_COUNT],
}

pub fn verify_scuba_soldier_routine_02(
    ctx: ScubaSoldierRoutine02Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::scuba_soldier::{scuba_soldier_routine_02, ScubaSoldierRoutine02Outcome};

    let x = ctx.x;
    let expected = scuba_soldier_routine_02(
        prg_rom,
        &ctx.enemy_routine_slots,
        ctx.current_level,
        ctx.var_1,
        ctx.animation_delay,
        ctx.attack_delay,
        ctx.state_width,
        ctx.level_scrolling_type,
        ctx.frame_scroll,
        ctx.x_pos,
        ctx.y_pos,
        ctx.routine,
    );
    *checked += 1;

    let real_sprite = bus.ram[0x30A + x];
    let real_var_1 = bus.ram[0x5B8 + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_animation_delay = bus.ram[0x538 + x];
    let real_attack_delay = bus.ram[0x558 + x];
    let real_state_width = bus.ram[0x598 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let base_ok =
        real_sprite == expected.sprite && real_var_1 == expected.var_1 && real_sprite_attr == expected.sprite_attr && real_x_pos == expected.scroll.x_pos && real_y_pos == expected.scroll.y_pos;

    let mismatch = !base_ok
        || match &expected.outcome {
            ScubaSoldierRoutine02Outcome::Firing { animation_delay, attack_delay } => real_animation_delay != *animation_delay || real_attack_delay != *attack_delay,
            ScubaSoldierRoutine02Outcome::Fired { animation_delay, mortar } => {
                let mortar_ok = match mortar {
                    Some(m) => {
                        let mx = m.slot as usize;
                        bus.ram[0x33E + mx] == m.x_pos && bus.ram[0x324 + mx] == m.y_pos && bus.ram[0x528 + mx] == 0x0B && bus.ram[0x4B8 + mx] == m.initialized.routine
                    }
                    None => true,
                };
                !mortar_ok || real_animation_delay != *animation_delay
            }
            ScubaSoldierRoutine02Outcome::Submerging { animation_delay, disabled_state_width, routine_update } => {
                real_animation_delay != *animation_delay || real_state_width != *disabled_state_width || real_routine != routine_update.routine
            }
        };

    if mismatch {
        eprintln!(
            "MISMATCH(scuba_soldier_routine_02) frame={frame} pc={:04X} in=(var_1={:02X} delay={:02X} attack_delay={:02X} x={:02X} y={:02X} routine={:02X}): expected {:?}, got sprite={real_sprite:02X} var_1={real_var_1:02X} sprite_attr={real_sprite_attr:02X} x={real_x_pos:02X} y={real_y_pos:02X} animation_delay={real_animation_delay:02X} attack_delay={real_attack_delay:02X} state_width={real_state_width:02X} routine={real_routine:02X}",
            cpu.pc, ctx.var_1, ctx.animation_delay, ctx.attack_delay, ctx.x_pos, ctx.y_pos, ctx.routine, expected.outcome
        );
    }
}
