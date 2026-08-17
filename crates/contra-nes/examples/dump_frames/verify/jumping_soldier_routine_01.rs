/// Captured inputs for one real `jumping_soldier_routine_01` call
/// (`$93a5`).
#[derive(Clone)]
pub struct JumpingSoldierRoutine01Ctx {
    pub x: usize,
    pub current_level: u8,
    pub attack_flag: u8,
    pub animation_delay: u8,
    pub attributes: u8,
    pub sprite_attr: u8,
    pub x_vel_accum: u8,
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub var_1: u8,
    pub player_state: [u8; 2],
    pub sprite_y_pos: [u8; 2],
    pub sprite_x_pos: [u8; 2],
    pub level_location_type: u8,
    pub enemy_routine: [u8; contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT],
}

pub fn verify_jumping_soldier_routine_01(
    ctx: JumpingSoldierRoutine01Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::indoor_soldier::ApplyEnemyVelocityOutcome;
    use contra_native::enemy::jumping_soldier::{jumping_soldier_routine_01, JumpingSoldierRoutine01Outcome};

    let x = ctx.x;
    let expected = jumping_soldier_routine_01(
        prg_rom,
        &ctx.enemy_routine,
        ctx.current_level,
        ctx.attack_flag,
        ctx.animation_delay,
        ctx.attributes,
        ctx.sprite_attr,
        ctx.x_vel_accum,
        ctx.x_vel_fract,
        ctx.x_vel_fast,
        ctx.x_pos,
        ctx.y_pos,
        ctx.var_1,
        ctx.player_state,
        ctx.sprite_y_pos,
        ctx.sprite_x_pos,
        ctx.level_location_type,
    );
    *checked += 1;

    let real_sprites = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_x_vel_accum = bus.ram[0x4D8 + x];
    let real_animation_delay = bus.ram[0x538 + x];
    let real_var_1 = bus.ram[0x5B8 + x];

    let mut mismatch = real_sprites != expected.sprites;

    match &expected.outcome {
        JumpingSoldierRoutine01Outcome::Waiting { animation_delay } => {
            mismatch = mismatch || real_sprite_attr != expected.sprite_attr || real_animation_delay != *animation_delay;
        }
        JumpingSoldierRoutine01Outcome::Fired { animation_delay, .. } => {
            mismatch = mismatch || real_sprite_attr != expected.sprite_attr || real_animation_delay != *animation_delay;
        }
        JumpingSoldierRoutine01Outcome::Jumping(j) => {
            let expected_sprite_attr = match j.velocity.outcome {
                ApplyEnemyVelocityOutcome::Removed(_) => expected.sprite_attr,
                ApplyEnemyVelocityOutcome::BgPriority(attr) => attr,
            };
            mismatch = mismatch
                || real_sprite_attr != expected_sprite_attr
                || real_x_pos != j.velocity.x_pos
                || real_x_vel_accum != j.velocity.x_vel_accum
                || real_y_pos != j.y_pos
                || real_var_1 != j.var_1
                || j.animation_delay.map(|d| real_animation_delay != d).unwrap_or(false);
        }
    }

    if mismatch {
        eprintln!(
            "MISMATCH(jumping_soldier_routine_01) frame={frame} pc={:04X} in=(delay={:02X} attrs={:02X} sprite_attr={:02X} x_vel=({:02X},{:02X},{:02X}) x={:02X} y={:02X} var_1={:02X}): expected {:?}, got sprites={real_sprites:02X} sprite_attr={real_sprite_attr:02X} x={real_x_pos:02X} y={real_y_pos:02X} x_vel_accum={real_x_vel_accum:02X} animation_delay={real_animation_delay:02X} var_1={real_var_1:02X}",
            cpu.pc,
            ctx.animation_delay,
            ctx.attributes,
            ctx.sprite_attr,
            ctx.x_vel_accum,
            ctx.x_vel_fract,
            ctx.x_vel_fast,
            ctx.x_pos,
            ctx.y_pos,
            ctx.var_1,
            expected
        );
    }
}
