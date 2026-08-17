/// Captured inputs for one real `soldier_routine_01` call (`$8665`),
/// snapshotted at entry - see `VERIFY_SOLDIER_ROUTINE_01`'s comment in
/// `main` for the real exits and the nested-`rts` disambiguation this
/// needs.
#[derive(Clone, Copy)]
pub struct SoldierRoutine01Ctx {
    pub x: usize,
    pub scroll_type: u8,
    pub frame_scroll: u8,
    pub frame_counter: u8,
    pub attrs: u8,
    pub delay: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub state_width: u8,
    pub vscroll: u8,
    pub hscroll: u8,
    pub ppuctrl: u8,
    pub data: [u8; contra_native::physics::collision::BG_COLLISION_DATA_LEN],
    pub routine: u8,
}

pub fn verify_soldier_routine_01(
    ctx: SoldierRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::soldier::{soldier_routine_01, SoldierRoutine01Outcome};

    let x = ctx.x;
    let expected = soldier_routine_01(
        ctx.scroll_type, ctx.frame_scroll, ctx.frame_counter, ctx.attrs, ctx.delay, ctx.x_pos, ctx.y_pos,
        ctx.state_width, ctx.vscroll, ctx.hscroll, ctx.ppuctrl, &ctx.data, ctx.routine,
    );
    *checked += 1;

    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_delay = bus.ram[0x538 + x];
    let real_routine = bus.ram[0x4B8 + x];
    let real_sprites = bus.ram[0x30A + x];
    let real_state_width = bus.ram[0x598 + x];
    let real_var_2 = bus.ram[0x5C8 + x];
    let real_x_fract = bus.ram[0x518 + x];
    let real_x_fast = bus.ram[0x508 + x];
    let real_y_fract = bus.ram[0x4F8 + x];
    let real_y_fast = bus.ram[0x4E8 + x];

    let expected_x_pos_before_advance = expected.scrolled.map(|s| s.x_pos).unwrap_or(ctx.x_pos);
    let expected_y_pos = expected.scrolled.map(|s| s.y_pos).unwrap_or(ctx.y_pos);

    let mismatch = match expected.outcome {
        SoldierRoutine01Outcome::NoDecrement => {
            real_x_pos != expected_x_pos_before_advance
                || real_y_pos != expected_y_pos
                || real_delay != ctx.delay
                || real_routine != ctx.routine
        }
        SoldierRoutine01Outcome::DelayNotYetZero { animation_delay, .. } => {
            real_x_pos != expected_x_pos_before_advance
                || real_y_pos != expected_y_pos
                || real_delay != animation_delay
                || real_routine != ctx.routine
        }
        SoldierRoutine01Outcome::Removed(_) => real_routine != 0 || real_sprites != 0,
        SoldierRoutine01Outcome::Advanced(a) => {
            real_x_pos != a.enemy_x_pos
                || real_y_pos != expected_y_pos
                || real_delay != a.delayed_routine.animation_delay
                || real_routine != a.delayed_routine.routine_update.routine
                || a.delayed_routine.routine_update.sprites.map(|s| s != real_sprites).unwrap_or(false)
                || real_state_width != a.state_width
                || real_var_2 != a.enemy_var_2
                || real_x_fract != a.velocity.x_velocity.0
                || real_x_fast != a.velocity.x_velocity.1
                || real_y_fract != a.velocity.y_velocity.vel_fract
                || real_y_fast != a.velocity.y_velocity.vel_fast
        }
    };

    if mismatch {
        eprintln!(
            "MISMATCH(soldier_routine_01) frame={frame} pc={:04X} in=(scroll_type={:02X} frame_scroll={:02X} frame_counter={:02X} attrs={:02X} delay={:02X} x={:02X} y={:02X} state_width={:02X} routine={:02X}): expected {:?}, got x={real_x_pos:02X} y={real_y_pos:02X} delay={real_delay:02X} routine={real_routine:02X} sprites={real_sprites:02X} state_width={real_state_width:02X} var_2={real_var_2:02X} xvel=({real_x_fract:02X},{real_x_fast:02X}) yvel=({real_y_fract:02X},{real_y_fast:02X})",
            cpu.pc, ctx.scroll_type, ctx.frame_scroll, ctx.frame_counter, ctx.attrs, ctx.delay, ctx.x_pos, ctx.y_pos, ctx.state_width, ctx.routine, expected
        );
    }
}

