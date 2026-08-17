/// Captured inputs for one real `soldier_routine_02` call whose
/// `ENEMY_VAR_3` was nonzero at entry (`$86af`) - the jumping sub-path
/// this crate ports (`soldier_routine_02_jumping`). See
/// `VERIFY_SOLDIER_ROUTINE_02_JUMPING`'s comment in `main` for the real
/// exits and the nested-`jsr`-return disambiguation this needs.
#[derive(Clone, Copy)]
pub struct SoldierRoutine02JumpingCtx {
    pub x: usize,
    pub var_3: u8,
    pub y_vel_fast: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub var_4: u8,
    pub var_2: u8,
    pub var_1: u8,
    pub vscroll: u8,
    pub hscroll: u8,
    pub ppuctrl: u8,
    pub data: [u8; contra_native::physics::collision::BG_COLLISION_DATA_LEN],
    pub scroll_type: u8,
    pub frame_scroll: u8,
    pub x_accum: u8,
    pub x_fract: u8,
    pub x_fast: u8,
    pub y_accum: u8,
    pub y_fract: u8,
    pub y_fast: u8,
    pub routine: u8,
}

pub fn verify_soldier_routine_02_jumping(
    ctx: SoldierRoutine02JumpingCtx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::soldier::{soldier_routine_02_jumping, SoldierApplyVelOutcome, SoldierRoutine02Landing};

    let x = ctx.x;
    let expected = soldier_routine_02_jumping(
        ctx.var_3, ctx.y_vel_fast, ctx.x_pos, ctx.y_pos, ctx.var_4, ctx.var_2, ctx.var_1, ctx.vscroll, ctx.hscroll,
        ctx.ppuctrl, &ctx.data, ctx.scroll_type, ctx.frame_scroll, ctx.x_accum, ctx.x_fract, ctx.x_fast, ctx.y_accum,
        ctx.y_fract, ctx.y_fast, ctx.routine,
    );
    *checked += 1;

    let real_var_3 = bus.ram[0x5D8 + x];
    let real_frame = bus.ram[0x568 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_var_2 = bus.ram[0x5C8 + x];
    let real_sprites = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_var_1 = bus.ram[0x5B8 + x];
    let real_x_fract = bus.ram[0x518 + x];
    let real_x_fast = bus.ram[0x508 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mut mismatch = real_var_3 != expected.enemy_var_3 || real_frame != expected.enemy_frame;

    // The water-landing case's own `jsr set_enemy_routine_to_a` already
    // committed a routine switch before the tail runs - fold that into
    // "effective routine" the same way `soldier_routine_02_jumping`
    // itself does internally, so the tail's own guard check (relevant
    // only for `SolidAtOwnPosition`) lines up with what real hardware
    // actually saw.
    let water_switch = match expected.landing {
        SoldierRoutine02Landing::NotLanded { water_routine_switch, .. } => water_routine_switch,
        SoldierRoutine02Landing::Landed { .. } => None,
    };

    match expected.tail {
        SoldierApplyVelOutcome::SolidAtOwnPosition(routine_update) => {
            mismatch |= real_routine != routine_update.routine;
            if let Some(s) = routine_update.sprites {
                mismatch |= real_sprites != s;
            }
        }
        SoldierApplyVelOutcome::Continued(result) => {
            let (expected_var_2, expected_x_fract, expected_x_fast) = match result.direction_change {
                Some(d) => (d.var_2, d.x_velocity.0, d.x_velocity.1),
                None => (ctx.var_2, ctx.x_fract, ctx.x_fast),
            };
            let removed = result.position.removed.is_some();
            let expected_routine = if removed {
                0
            } else {
                water_switch.map(|u| u.routine).unwrap_or(ctx.routine)
            };
            let expected_sprites = if removed { 0 } else { result.sprite.sprite };

            mismatch |= real_x_pos != result.position.x.pos
                || real_y_pos != result.position.y.pos
                || real_var_2 != expected_var_2
                || real_x_fract != expected_x_fract
                || real_x_fast != expected_x_fast
                || real_sprite_attr != result.sprite.sprite_attr
                || real_var_1 != result.sprite.var_1
                || real_sprites != expected_sprites
                || real_routine != expected_routine;
        }
    }

    if mismatch {
        eprintln!(
            "MISMATCH(soldier_routine_02_jumping) frame={frame} pc={:04X} in=(var_3={:02X} y_vel_fast={:02X} x={:02X} y={:02X} var_4={:02X} var_2={:02X} var_1={:02X} scroll_type={:02X} frame_scroll={:02X} routine={:02X}): expected {:?}, got x={real_x_pos:02X} y={real_y_pos:02X} var_3={real_var_3:02X} frame={real_frame:02X} var_2={real_var_2:02X} var_1={real_var_1:02X} sprites={real_sprites:02X} sprite_attr={real_sprite_attr:02X} xvel=({real_x_fract:02X},{real_x_fast:02X}) routine={real_routine:02X}",
            cpu.pc, ctx.var_3, ctx.y_vel_fast, ctx.x_pos, ctx.y_pos, ctx.var_4, ctx.var_2, ctx.var_1, ctx.scroll_type, ctx.frame_scroll, ctx.routine, expected
        );
    }
}

