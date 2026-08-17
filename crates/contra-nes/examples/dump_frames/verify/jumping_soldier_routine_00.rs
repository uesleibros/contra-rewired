/// Captured inputs for one real `jumping_soldier_routine_00` call
/// (`$9380`).
#[derive(Clone, Copy)]
pub struct JumpingSoldierRoutine00Ctx {
    pub x: usize,
    pub attributes: u8,
    pub indoor_red_soldier_created: u8,
    pub indoor_enemy_attack_count: u8,
    pub routine: u8,
}

pub fn verify_jumping_soldier_routine_00(
    ctx: JumpingSoldierRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::jumping_soldier::jumping_soldier_routine_00;

    let x = ctx.x;
    let expected = jumping_soldier_routine_00(ctx.attributes, ctx.indoor_red_soldier_created, ctx.indoor_enemy_attack_count, ctx.routine);
    *checked += 1;

    let real_attributes = bus.ram[0x5A8 + x];
    let real_created_flag = bus.ram[0x89];
    let real_x_fract = bus.ram[0x518 + x];
    let real_x_fast = bus.ram[0x508 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let expected_created_flag = expected.indoor_red_soldier_created.unwrap_or(ctx.indoor_red_soldier_created);

    let mismatch = real_attributes != expected.attributes
        || real_created_flag != expected_created_flag
        || real_x_fract != expected.init.x_velocity.0
        || real_x_fast != expected.init.x_velocity.1
        || real_x_pos != expected.init.x_pos
        || real_y_pos != expected.init.y_pos
        || real_routine != expected.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(jumping_soldier_routine_00) frame={frame} pc={:04X} in=(attrs={:02X} created={:02X} attack_count={:02X} routine={:02X}): expected {:?}, got attrs={real_attributes:02X} created={real_created_flag:02X} xvel=({real_x_fract:02X},{real_x_fast:02X}) x={real_x_pos:02X} y={real_y_pos:02X} routine={real_routine:02X}",
            cpu.pc, ctx.attributes, ctx.indoor_red_soldier_created, ctx.indoor_enemy_attack_count, ctx.routine, expected
        );
    }
}
