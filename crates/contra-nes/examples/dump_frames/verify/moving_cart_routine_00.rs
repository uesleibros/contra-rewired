/// Captured inputs for one real `moving_cart_routine_00` call (`$b186`).
#[derive(Clone, Copy)]
pub struct MovingCartRoutine00Ctx {
    pub x: usize,
    pub frame_counter: u8,
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
    pub var_4: u8,
    pub attributes: u8,
    pub vertical_scroll: u8,
    pub horizontal_scroll: u8,
    pub ppuctrl_settings: u8,
    pub bg_collision_data: [u8; contra_native::physics::collision::BG_COLLISION_DATA_LEN],
    pub routine: u8,
}

pub fn verify_moving_cart_routine_00(
    ctx: MovingCartRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::mine_cart::{moving_cart_routine_00, MovingCartCollisionOutcome, MovingCartRoutine00Outcome};

    let x = ctx.x;
    let expected = moving_cart_routine_00(
        ctx.frame_counter,
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
        ctx.var_4,
        ctx.attributes,
        ctx.vertical_scroll,
        ctx.horizontal_scroll,
        ctx.ppuctrl_settings,
        &ctx.bg_collision_data,
        ctx.routine,
    );
    *checked += 1;

    let real_sprite = bus.ram[0x30A + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_x_vel_accum = bus.ram[0x4D8 + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_y_vel_accum = bus.ram[0x4C8 + x];
    let real_x_vel_fract = bus.ram[0x518 + x];
    let real_x_vel_fast = bus.ram[0x508 + x];
    let real_var_4 = bus.ram[0x5E8 + x];
    let real_y_vel_fract = bus.ram[0x4F8 + x];
    let real_y_vel_fast = bus.ram[0x4E8 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let base_ok = real_sprite == expected.sprite
        && real_x_pos == expected.position.x.pos
        && real_x_vel_accum == expected.position.x.vel_accum
        && real_y_pos == expected.position.y.pos
        && real_y_vel_accum == expected.position.y.vel_accum;

    let outcome_ok = match &expected.outcome {
        MovingCartRoutine00Outcome::CollisionAhead(MovingCartCollisionOutcome::Explodes(update)) => real_routine == update.routine,
        MovingCartRoutine00Outcome::CollisionAhead(MovingCartCollisionOutcome::ReversesDirection { var_4, x_vel_fract, x_vel_fast }) => {
            real_var_4 == *var_4 && real_x_vel_fract == *x_vel_fract && real_x_vel_fast == *x_vel_fast
        }
        MovingCartRoutine00Outcome::OnTrack => true,
        MovingCartRoutine00Outcome::Falling { y_vel_fract, y_vel_fast } => real_y_vel_fract == *y_vel_fract && real_y_vel_fast == *y_vel_fast,
    };

    let mismatch = !base_ok || !outcome_ok;

    if mismatch {
        eprintln!(
            "MISMATCH(moving_cart_routine_00) frame={frame} pc={:04X} in=(x={:02X} y={:02X} var_4={:02X} attrs={:02X} routine={:02X}): expected {:?}, got sprite={real_sprite:02X} x={real_x_pos:02X} y={real_y_pos:02X} var_4={real_var_4:02X} x_vel_fract={real_x_vel_fract:02X} x_vel_fast={real_x_vel_fast:02X} y_vel_fract={real_y_vel_fract:02X} y_vel_fast={real_y_vel_fast:02X} routine={real_routine:02X}",
            cpu.pc, ctx.x_pos, ctx.y_pos, ctx.var_4, ctx.attributes, ctx.routine, expected
        );
    }
}
