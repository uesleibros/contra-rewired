/// Captured inputs for one real `mortar_shot_routine_01` call (`$f237`).
#[derive(Clone, Copy)]
pub struct MortarShotRoutine01Ctx {
    pub x: usize,
    pub enemy_attributes: u8,
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
    pub sprite_x_pos: [u8; 2],
    pub sprite_y_pos: [u8; 2],
    pub player_state: [u8; 2],
    pub vertical_scroll: u8,
    pub horizontal_scroll: u8,
    pub ppuctrl_settings: u8,
    pub bg_collision_data: [u8; contra_native::physics::collision::BG_COLLISION_DATA_LEN],
    pub routine: u8,
}

pub fn verify_mortar_shot_routine_01(
    ctx: MortarShotRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::mortar_shot::{mortar_shot_routine_01, MortarShotRoutine01Outcome};

    let x = ctx.x;
    let expected = mortar_shot_routine_01(
        ctx.enemy_attributes,
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
        ctx.sprite_x_pos,
        ctx.sprite_y_pos,
        ctx.player_state,
        ctx.vertical_scroll,
        ctx.horizontal_scroll,
        ctx.ppuctrl_settings,
        &ctx.bg_collision_data,
        ctx.routine,
    );
    *checked += 1;

    let real_y_vel_fract = bus.ram[0x4F8 + x];
    let real_y_vel_fast = bus.ram[0x4E8 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_x_vel_accum = bus.ram[0x4D8 + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_y_vel_accum = bus.ram[0x4C8 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let base_ok = real_y_vel_fract == expected.y_vel_fract
        && real_y_vel_fast == expected.y_vel_fast
        && real_x_pos == expected.position.x.pos
        && real_x_vel_accum == expected.position.x.vel_accum
        && real_y_pos == expected.position.y.pos
        && real_y_vel_accum == expected.position.y.vel_accum;

    let mismatch = !base_ok
        || match &expected.outcome {
            MortarShotRoutine01Outcome::StillRising | MortarShotRoutine01Outcome::SplitStillRising | MortarShotRoutine01Outcome::SplitAboveClosestPlayer | MortarShotRoutine01Outcome::SplitNoBgCollision => false,
            MortarShotRoutine01Outcome::Advanced(update) => real_routine != update.routine,
            MortarShotRoutine01Outcome::SplitCollided { routine_update, .. } => real_routine != routine_update.routine,
        };

    if mismatch {
        eprintln!(
            "MISMATCH(mortar_shot_routine_01) frame={frame} pc={:04X} in=(attrs={:02X} x={:02X} y={:02X} routine={:02X}): expected {:?}, got y_fract={real_y_vel_fract:02X} y_fast={real_y_vel_fast:02X} x={real_x_pos:02X} y={real_y_pos:02X} routine={real_routine:02X}",
            cpu.pc, ctx.enemy_attributes, ctx.x_pos, ctx.y_pos, ctx.routine, expected
        );
    }
}
