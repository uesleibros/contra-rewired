/// Captured inputs for one real `flying_capsule_routine_00` call
/// (`$830b`).
#[derive(Clone, Copy)]
pub struct FlyingCapsuleRoutine00Ctx {
    pub x: usize,
    pub level_scrolling_type: u8,
    pub y_pos: u8,
    pub x_pos: u8,
    pub routine: u8,
}

pub fn verify_flying_capsule_routine_00(
    ctx: FlyingCapsuleRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::flying_capsule::{flying_capsule_routine_00, FlyingCapsuleRoutine00Outcome};

    let x = ctx.x;
    let expected = flying_capsule_routine_00(ctx.level_scrolling_type, ctx.y_pos, ctx.x_pos, ctx.routine);
    *checked += 1;

    let real_sprite_attr = bus.ram[0x358 + x];
    let real_var_1 = bus.ram[0x5B8 + x];
    let real_var_2 = bus.ram[0x5C8 + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_fract = bus.ram[0x4F8 + x];
    let real_y_fast = bus.ram[0x4E8 + x];
    let real_x_fract = bus.ram[0x518 + x];
    let real_x_fast = bus.ram[0x508 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let (expected_y_pos, expected_x_pos) = match expected.outcome {
        FlyingCapsuleRoutine00Outcome::Horizontal { y_pos, x_pos } => (y_pos, x_pos),
        FlyingCapsuleRoutine00Outcome::Vertical { x_pos, y_pos } => (y_pos, x_pos),
    };

    let mismatch = real_sprite_attr != expected.sprite_attr
        || real_var_1 != expected.var_1
        || real_var_2 != expected.var_2
        || real_y_pos != expected_y_pos
        || real_x_pos != expected_x_pos
        || (real_y_fract, real_y_fast) != expected.y_velocity
        || (real_x_fract, real_x_fast) != expected.x_velocity
        || real_routine != expected.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(flying_capsule_routine_00) frame={frame} pc={:04X} in=(scroll_type={:02X} y={:02X} x={:02X} routine={:02X}): expected {:?}, got sprite_attr={real_sprite_attr:02X} var_1={real_var_1:02X} var_2={real_var_2:02X} y={real_y_pos:02X} x={real_x_pos:02X} yvel=({real_y_fract:02X},{real_y_fast:02X}) xvel=({real_x_fract:02X},{real_x_fast:02X}) routine={real_routine:02X}",
            cpu.pc, ctx.level_scrolling_type, ctx.y_pos, ctx.x_pos, ctx.routine, expected
        );
    }
}
