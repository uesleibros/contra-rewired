use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

/// Captured inputs for one real `mortar_shot_routine_02` call (`$f26e`).
#[derive(Clone, Copy)]
pub struct MortarShotRoutine02Ctx {
    pub x: usize,
    pub current_level: u8,
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
    pub routine: u8,
    pub enemy_routine_slots: [u8; ENEMY_SLOT_COUNT],
}

pub fn verify_mortar_shot_routine_02(
    ctx: MortarShotRoutine02Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::mortar_shot::mortar_shot_routine_02;

    let x = ctx.x;
    let expected = mortar_shot_routine_02(
        prg_rom,
        ctx.enemy_routine_slots,
        ctx.current_level,
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
        ctx.routine,
    );
    *checked += 1;

    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let pos_ok = real_x_pos == expected.position.x.pos && real_y_pos == expected.position.y.pos;
    let splits_ok = expected.splits.iter().all(|s| {
        let sx = s.slot as usize;
        bus.ram[0x33E + sx] == s.x_pos && bus.ram[0x324 + sx] == s.y_pos && bus.ram[0x528 + sx] == 0x0B && bus.ram[0x5A8 + sx] == s.attributes && bus.ram[0x4B8 + sx] == s.initialized.routine
    });

    let mismatch = !pos_ok || !splits_ok || real_routine != expected.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(mortar_shot_routine_02) frame={frame} pc={:04X} in=(x={:02X} y={:02X} routine={:02X}): expected {:?}, got x={real_x_pos:02X} y={real_y_pos:02X} routine={real_routine:02X}",
            cpu.pc, ctx.x_pos, ctx.y_pos, ctx.routine, expected
        );
    }
}
