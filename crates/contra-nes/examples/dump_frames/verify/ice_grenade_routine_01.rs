/// Captured inputs for one real `ice_grenade_routine_01` call (`$a3d7`).
#[derive(Clone, Copy)]
pub struct IceGrenadeRoutine01Ctx {
    pub x: usize,
    pub frame_counter: u8,
    pub enemy_frame: u8,
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
    pub vertical_scroll: u8,
    pub horizontal_scroll: u8,
    pub ppuctrl_settings: u8,
    pub bg_collision_data: [u8; contra_native::physics::collision::BG_COLLISION_DATA_LEN],
    pub routine: u8,
}

pub fn verify_ice_grenade_routine_01(
    ctx: IceGrenadeRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::ice::{ice_grenade_routine_01, IceGrenadeRoutine01Outcome};

    let x = ctx.x;
    let expected = ice_grenade_routine_01(
        ctx.frame_counter,
        ctx.enemy_frame,
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
        ctx.vertical_scroll,
        ctx.horizontal_scroll,
        ctx.ppuctrl_settings,
        &ctx.bg_collision_data,
        ctx.routine,
    );
    *checked += 1;

    let real_frame = bus.ram[0x568 + x];
    let real_sprite = bus.ram[0x30A + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_y_vel_fract = bus.ram[0x4F8 + x];
    let real_y_vel_fast = bus.ram[0x4E8 + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let base_ok = real_frame == expected.frame
        && real_sprite == expected.sprite
        && real_x_pos == expected.position.x.pos
        && real_y_pos == expected.position.y.pos
        && real_y_vel_fract == expected.y_vel_fract
        && real_y_vel_fast == expected.y_vel_fast;

    let outcome_ok = match expected.outcome {
        IceGrenadeRoutine01Outcome::StillFalling => true,
        IceGrenadeRoutine01Outcome::NoGroundYet { sprite_attr } => real_sprite_attr == sprite_attr,
        IceGrenadeRoutine01Outcome::Exploding { sprite_attr, routine_update, .. } => real_sprite_attr == sprite_attr && real_routine == routine_update.routine,
    };

    let mismatch = !base_ok || !outcome_ok;

    if mismatch {
        eprintln!(
            "MISMATCH(ice_grenade_routine_01) frame={frame} pc={:04X} in=(fc={:02X} ef={:02X} x={:02X} y={:02X} routine={:02X}): expected {:?}, got frame={real_frame:02X} sprite={real_sprite:02X} x={real_x_pos:02X} y={real_y_pos:02X} y_vel_fract={real_y_vel_fract:02X} y_vel_fast={real_y_vel_fast:02X} sprite_attr={real_sprite_attr:02X} routine={real_routine:02X}",
            cpu.pc, ctx.frame_counter, ctx.enemy_frame, ctx.x_pos, ctx.y_pos, ctx.routine, expected
        );
    }
}
