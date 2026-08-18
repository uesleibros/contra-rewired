/// Captured inputs for one real `sniper_routine_01` call (`$8982`).
#[derive(Clone, Copy)]
pub struct SniperRoutine01Ctx {
    pub x: usize,
    pub sniper_type: u8,
    pub frame: u8,
    pub var_2: u8,
    pub var_3: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub animation_delay: u8,
    pub state_width: u8,
    pub routine: u8,
}

pub fn verify_sniper_routine_01(
    ctx: SniperRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::sniper::{sniper_routine_01, ActivatedFrom, SniperRoutine01Outcome};

    let x = ctx.x;
    let expected = sniper_routine_01(
        ctx.sniper_type,
        ctx.frame,
        ctx.var_2,
        ctx.var_3,
        ctx.level_scrolling_type,
        ctx.frame_scroll,
        ctx.x_pos,
        ctx.y_pos,
        ctx.animation_delay,
        ctx.state_width,
        ctx.routine,
    );
    *checked += 1;

    let real_sprites = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_var_3 = bus.ram[0x5D8 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_animation_delay = bus.ram[0x538 + x];
    let real_frame = bus.ram[0x568 + x];
    let real_state_width = bus.ram[0x598 + x];
    let real_score_collision = bus.ram[0x588 + x];
    let real_attack_delay = bus.ram[0x558 + x];
    let real_var_4 = bus.ram[0x5E8 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let sprite_ok = real_sprites == expected.sprite.sprites && real_sprite_attr == expected.sprite.sprite_attr && real_var_3 == expected.sprite.var_3;
    let pos_ok = real_x_pos == expected.scroll.x_pos && real_y_pos == expected.scroll.y_pos;

    let mismatch = !sprite_ok
        || !pos_ok
        || match expected.outcome {
            SniperRoutine01Outcome::Waiting { animation_delay } => real_animation_delay != animation_delay,
            SniperRoutine01Outcome::CrouchCycling { animation_delay, frame: f } => real_animation_delay != animation_delay || real_frame != f,
            SniperRoutine01Outcome::Activated { from, frame: f, state_width, score_collision, attack_delay, var_4, routine_update } => {
                let nudge_ok = match from {
                    ActivatedFrom::BossNudge { y_pos, x_pos } | ActivatedFrom::CrouchFallthroughNudge { y_pos, x_pos } => {
                        real_y_pos == y_pos && real_x_pos == x_pos
                    }
                    ActivatedFrom::Standing | ActivatedFrom::Crouching => true,
                };
                !nudge_ok
                    || real_frame != f
                    || real_state_width != state_width
                    || real_score_collision != score_collision
                    || real_attack_delay != attack_delay
                    || real_var_4 != var_4
                    || real_routine != routine_update.routine
            }
        };

    if mismatch {
        eprintln!(
            "MISMATCH(sniper_routine_01) frame={frame} pc={:04X} in=(type={:02X} frame_num={:02X} var_2={:02X} var_3={:02X} x={:02X} y={:02X} delay={:02X} routine={:02X}): expected {:?}, got sprites={real_sprites:02X} sprite_attr={real_sprite_attr:02X} var_3={real_var_3:02X} x={real_x_pos:02X} y={real_y_pos:02X} animation_delay={real_animation_delay:02X} frame_num={real_frame:02X} state_width={real_state_width:02X} score_collision={real_score_collision:02X} attack_delay={real_attack_delay:02X} var_4={real_var_4:02X} routine={real_routine:02X}",
            cpu.pc, ctx.sniper_type, ctx.frame, ctx.var_2, ctx.var_3, ctx.x_pos, ctx.y_pos, ctx.animation_delay, ctx.routine, expected
        );
    }
}
