/// Captured inputs for one real `grenade_routine_01` call (`$8fe8`).
#[derive(Clone, Copy)]
pub struct GrenadeRoutine01Ctx {
    pub x: usize,
    pub var_1: u8,
    pub var_2: u8,
    pub var_3: u8,
    pub var_4: u8,
    pub var_b: u8,
    pub enemy_frame: u8,
    pub frame_counter: u8,
    pub attack_delay: u8,
    pub y_vel_accum: u8,
    pub y_vel_fract: u8,
    pub y_vel_fast: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub x_vel_accum: u8,
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    pub routine: u8,
}

pub fn verify_grenade_routine_01(
    ctx: GrenadeRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::enemy_falling_arc::SetEnemyFallingArcPosOutcome;
    use contra_native::enemy::grenade::grenade_routine_01;

    let x = ctx.x;
    let expected = grenade_routine_01(
        ctx.var_1,
        ctx.var_2,
        ctx.var_3,
        ctx.var_4,
        ctx.var_b,
        ctx.enemy_frame,
        ctx.frame_counter,
        ctx.attack_delay,
        ctx.y_vel_accum,
        ctx.y_vel_fract,
        ctx.y_vel_fast,
        ctx.frame_scroll,
        ctx.x_pos,
        ctx.x_vel_accum,
        ctx.x_vel_fract,
        ctx.x_vel_fast,
        ctx.routine,
    );
    *checked += 1;

    let real_frame = bus.ram[0x568 + x];
    let real_sprite = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_var_4 = bus.ram[0x5E8 + x];
    let real_attack_delay = bus.ram[0x558 + x];
    let real_var_1 = bus.ram[0x5B8 + x];
    let real_var_2 = bus.ram[0x5C8 + x];
    let real_var_3 = bus.ram[0x5D8 + x];
    let real_y_vel_accum = bus.ram[0x4C8 + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_x_vel_accum = bus.ram[0x4D8 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let base_ok = real_frame == expected.frame
        && real_sprite == expected.sprite
        && real_sprite_attr == expected.sprite_attr
        && real_var_4 == expected.var_4
        && real_attack_delay == expected.attack_delay
        && real_var_1 == expected.arc.vars.var_1
        && real_var_2 == expected.arc.vars.var_2
        && real_var_3 == expected.arc.vars.var_3
        && real_y_vel_accum == expected.arc.vars.y_vel_accum;

    let position_ok = match expected.arc.outcome {
        SetEnemyFallingArcPosOutcome::RemovedFallenOffBottom(_) => true,
        SetEnemyFallingArcPosOutcome::RemovedOffScreenLeft { y_pos, .. } => real_y_pos == y_pos,
        SetEnemyFallingArcPosOutcome::Position { y_pos, x } => real_y_pos == y_pos && real_x_pos == x.pos && real_x_vel_accum == x.vel_accum,
    };

    let routine_ok = match expected.routine_update {
        Some(update) => real_routine == update.routine,
        None => true,
    };

    let mismatch = !base_ok || !position_ok || !routine_ok;

    if mismatch {
        eprintln!(
            "MISMATCH(grenade_routine_01) frame={frame} pc={:04X} in=(var_1={:02X} var_2={:02X} var_3={:02X} var_4={:02X} frame={:02X} routine={:02X}): expected {:?}, got frame={real_frame:02X} sprite={real_sprite:02X} sprite_attr={real_sprite_attr:02X} var_1={real_var_1:02X} var_2={real_var_2:02X} var_3={real_var_3:02X} var_4={real_var_4:02X} y={real_y_pos:02X} x={real_x_pos:02X} routine={real_routine:02X}",
            cpu.pc, ctx.var_1, ctx.var_2, ctx.var_3, ctx.var_4, ctx.enemy_frame, ctx.routine, expected
        );
    }
}
