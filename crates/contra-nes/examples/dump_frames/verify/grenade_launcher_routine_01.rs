/// Captured inputs for one real `grenade_launcher_routine_01` call
/// (`$9479`).
#[derive(Clone)]
pub struct GrenadeLauncherRoutine01Ctx {
    pub x: usize,
    pub current_level: u8,
    pub attack_flag: u8,
    pub var_3: u8,
    pub animation_delay: u8,
    pub attack_delay: u8,
    pub var_1: u8,
    pub attributes: u8,
    pub frame_counter: u8,
    pub frame: u8,
    pub sprite_attr: u8,
    pub x_vel_accum: u8,
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub var_2: u8,
    pub player_state: [u8; 2],
    pub sprite_x_pos: [u8; 2],
    pub enemy_routine: [u8; contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT],
}

pub fn verify_grenade_launcher_routine_01(
    ctx: GrenadeLauncherRoutine01Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::grenade_launcher::{
        grenade_launcher_routine_01, GrenadeLauncherApplyVelAimOutcome, GrenadeLauncherCooldownOutcome, GrenadeLauncherRoutine01Outcome, LaunchGrenadeOutcome,
    };
    use contra_native::enemy::indoor_soldier::ApplyEnemyVelocityOutcome;

    let x = ctx.x;
    let expected = grenade_launcher_routine_01(
        prg_rom,
        &ctx.enemy_routine,
        ctx.current_level,
        ctx.attack_flag,
        ctx.var_3,
        ctx.animation_delay,
        ctx.attack_delay,
        ctx.var_1,
        ctx.attributes,
        ctx.frame_counter,
        ctx.frame,
        ctx.sprite_attr,
        ctx.x_vel_accum,
        ctx.x_vel_fract,
        ctx.x_vel_fast,
        ctx.x_pos,
        ctx.y_pos,
        ctx.var_2,
        ctx.player_state,
        ctx.sprite_x_pos,
    );
    *checked += 1;

    let real_sprites = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_x_vel_accum = bus.ram[0x4D8 + x];
    let real_x_vel_fract = bus.ram[0x518 + x];
    let real_x_vel_fast = bus.ram[0x508 + x];
    let real_animation_delay = bus.ram[0x538 + x];
    let real_attack_delay = bus.ram[0x558 + x];
    let real_var_1 = bus.ram[0x5B8 + x];
    let real_var_2 = bus.ram[0x5C8 + x];
    let real_var_3 = bus.ram[0x5D8 + x];

    let mismatch = match &expected {
        GrenadeLauncherRoutine01Outcome::ApplyVelAim(r) => {
            let sprite_ok = real_sprites == r.sprite.sprites;
            match r.outcome {
                GrenadeLauncherApplyVelAimOutcome::StillMoving { velocity, animation_delay } => {
                    let expected_attr = match velocity.outcome {
                        ApplyEnemyVelocityOutcome::Removed(_) => r.sprite.sprite_attr,
                        ApplyEnemyVelocityOutcome::BgPriority(a) => a,
                    };
                    !sprite_ok
                        || real_sprite_attr != expected_attr
                        || real_x_pos != velocity.x_pos
                        || real_x_vel_accum != velocity.x_vel_accum
                        || real_animation_delay != animation_delay
                }
                GrenadeLauncherApplyVelAimOutcome::Aimed { velocity, result } => {
                    let (expected_attr, x_ok, accum_ok) = match velocity {
                        Some(v) => {
                            let attr = match v.outcome {
                                ApplyEnemyVelocityOutcome::Removed(_) => r.sprite.sprite_attr,
                                ApplyEnemyVelocityOutcome::BgPriority(a) => a,
                            };
                            (attr, real_x_pos == v.x_pos, real_x_vel_accum == v.x_vel_accum)
                        }
                        None => (r.sprite.sprite_attr, real_x_pos == ctx.x_pos, true),
                    };
                    !sprite_ok
                        || real_sprite_attr != expected_attr
                        || !x_ok
                        || !accum_ok
                        || real_animation_delay != result.animation_delay
                        || real_var_3 != result.var_3
                        || real_attack_delay != result.attack_delay
                        || real_var_1 != result.var_1
                }
            }
        }
        GrenadeLauncherRoutine01Outcome::Cooldown { sprites, outcome } => {
            let sprite_ok = real_sprites == *sprites;
            match outcome {
                GrenadeLauncherCooldownOutcome::LaunchCheck { animation_delay, launch } => {
                    let launch_ok = match launch {
                        LaunchGrenadeOutcome::NotReady => true,
                        LaunchGrenadeOutcome::Waiting { attack_delay } => real_attack_delay == *attack_delay,
                        LaunchGrenadeOutcome::Launched { attack_delay, var_1, .. } => {
                            real_attack_delay == *attack_delay && real_var_1 == *var_1
                        }
                    };
                    !sprite_ok || real_animation_delay != *animation_delay || !launch_ok
                }
                GrenadeLauncherCooldownOutcome::Redirected { animation_delay, var_3, var_2, x_velocity } => {
                    let vel_ok = match x_velocity {
                        Some((fr, fa)) => real_x_vel_fract == *fr && real_x_vel_fast == *fa,
                        None => true,
                    };
                    !sprite_ok || real_animation_delay != *animation_delay || real_var_3 != *var_3 || real_var_2 != *var_2 || !vel_ok
                }
            }
        }
    };

    if mismatch {
        eprintln!(
            "MISMATCH(grenade_launcher_routine_01) frame={frame} pc={:04X} in=(var_3={:02X} delay={:02X} attack_delay={:02X} var_1={:02X} attrs={:02X} x={:02X} var_2={:02X}): expected {:?}, got sprites={real_sprites:02X} sprite_attr={real_sprite_attr:02X} x={real_x_pos:02X} x_vel=({real_x_vel_fract:02X},{real_x_vel_fast:02X}) animation_delay={real_animation_delay:02X} attack_delay={real_attack_delay:02X} var_1={real_var_1:02X} var_2={real_var_2:02X} var_3={real_var_3:02X}",
            cpu.pc, ctx.var_3, ctx.animation_delay, ctx.attack_delay, ctx.var_1, ctx.attributes, ctx.x_pos, ctx.var_2, expected
        );
    }
}
