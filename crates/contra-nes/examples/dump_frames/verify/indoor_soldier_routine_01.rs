/// Captured inputs for one real `indoor_soldier_routine_01` call
/// (`$92d5`).
#[derive(Clone)]
pub struct IndoorSoldierRoutine01Ctx {
    pub x: usize,
    pub current_level: u8,
    pub frame_counter: u8,
    pub enemy_frame: u8,
    pub enemy_sprite_attr: u8,
    pub x_vel_accum: u8,
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub attack_delay: u8,
    pub attributes: u8,
    pub var_1: u8,
    pub attack_flag: u8,
    pub attributes_scratch: u8,
    pub enemy_routine: [u8; contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT],
}

pub fn verify_indoor_soldier_routine_01(
    ctx: IndoorSoldierRoutine01Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::indoor_soldier::{indoor_soldier_routine_01, ApplyEnemyVelocityOutcome, IndoorSoldierAttack};

    let x = ctx.x;
    let expected = indoor_soldier_routine_01(
        prg_rom,
        &ctx.enemy_routine,
        ctx.current_level,
        ctx.frame_counter,
        ctx.enemy_frame,
        ctx.enemy_sprite_attr,
        ctx.x_vel_accum,
        ctx.x_vel_fract,
        ctx.x_vel_fast,
        ctx.x_pos,
        ctx.y_pos,
        ctx.attack_delay,
        ctx.attributes,
        ctx.var_1,
        ctx.attack_flag,
        ctx.attributes_scratch,
    );
    *checked += 1;

    let real_sprites = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_x_vel_accum = bus.ram[0x4D8 + x];
    let real_attack_delay = bus.ram[0x558 + x];
    let real_frame = bus.ram[0x568 + x];
    let real_var_1 = bus.ram[0x5B8 + x];

    // `apply_enemy_velocity_set_bg_priority`'s off-screen removal path
    // (`jmp remove_enemy`) re-zeroes ENEMY_SPRITES, overwriting `init_
    // sprite_from_frame`'s own just-written value back to 0 - but never
    // touches ENEMY_SPRITE_ATTR, which keeps `init_sprite_from_frame`'s
    // own value untouched (see `contra_native::enemy::indoor_soldier`'s
    // module doc comment on this same "keeps going after a same-frame
    // removal" quirk).
    let (expected_sprites, expected_sprite_attr) = match expected.velocity.outcome {
        ApplyEnemyVelocityOutcome::Removed(r) => (r.sprites, expected.sprite.sprite_attr),
        ApplyEnemyVelocityOutcome::BgPriority(attr) => (expected.sprite.sprites, attr),
    };

    let mut mismatch = real_sprites != expected_sprites
        || real_sprite_attr != expected_sprite_attr
        || real_x_pos != expected.velocity.x_pos
        || real_x_vel_accum != expected.velocity.x_vel_accum
        || real_attack_delay != expected.attack_delay
        || real_frame != expected.sprite.enemy_frame;

    match &expected.attack {
        IndoorSoldierAttack::StillWaiting | IndoorSoldierAttack::OutOfRange => {}
        IndoorSoldierAttack::Bullet(_) | IndoorSoldierAttack::Roller(_) => {}
        IndoorSoldierAttack::GrenadeSkipped { var_1 } | IndoorSoldierAttack::Grenade { var_1, .. } => {
            mismatch = mismatch || real_var_1 != *var_1;
        }
    }

    if mismatch {
        eprintln!(
            "MISMATCH(indoor_soldier_routine_01) frame={frame} pc={:04X} in=(frame_counter={:02X} enemy_frame={:02X} sprite_attr={:02X} x_vel=({:02X},{:02X},{:02X}) x={:02X} y={:02X} attack_delay={:02X} attrs={:02X} var_1={:02X} attack_flag={:02X}): expected {:?}, got sprites={real_sprites:02X} sprite_attr={real_sprite_attr:02X} x={real_x_pos:02X} x_vel_accum={real_x_vel_accum:02X} attack_delay={real_attack_delay:02X} frame={real_frame:02X} var_1={real_var_1:02X}",
            cpu.pc,
            ctx.frame_counter,
            ctx.enemy_frame,
            ctx.enemy_sprite_attr,
            ctx.x_vel_accum,
            ctx.x_vel_fract,
            ctx.x_vel_fast,
            ctx.x_pos,
            ctx.y_pos,
            ctx.attack_delay,
            ctx.attributes,
            ctx.var_1,
            ctx.attack_flag,
            expected
        );
    }
}
