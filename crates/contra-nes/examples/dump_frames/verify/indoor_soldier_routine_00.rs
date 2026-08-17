/// Captured inputs for one real `indoor_soldier_routine_00` call
/// (`$92c8`).
#[derive(Clone, Copy)]
pub struct IndoorSoldierRoutine00Ctx {
    pub x: usize,
    pub attributes: u8,
    pub routine: u8,
}

pub fn verify_indoor_soldier_routine_00(
    ctx: IndoorSoldierRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::indoor_soldier::indoor_soldier_routine_00;

    let x = ctx.x;
    let expected = indoor_soldier_routine_00(ctx.attributes, ctx.routine);
    *checked += 1;

    let real_x_fract = bus.ram[0x518 + x];
    let real_x_fast = bus.ram[0x508 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_attack_delay = bus.ram[0x558 + x];
    let real_routine = bus.ram[0x4B8 + x];
    let real_sprites = bus.ram[0x30A + x];

    let mismatch = real_x_fract != expected.init.x_velocity.0
        || real_x_fast != expected.init.x_velocity.1
        || real_x_pos != expected.init.x_pos
        || real_y_pos != expected.init.y_pos
        || real_attack_delay != expected.attack_delay
        || real_routine != expected.routine_update.routine
        || expected.routine_update.sprites.map(|s| real_sprites != s).unwrap_or(false);

    if mismatch {
        eprintln!(
            "MISMATCH(indoor_soldier_routine_00) frame={frame} pc={:04X} in=(attrs={:02X} routine={:02X}): expected {:?}, got xvel=({real_x_fract:02X},{real_x_fast:02X}) x={real_x_pos:02X} y={real_y_pos:02X} attack_delay={real_attack_delay:02X} routine={real_routine:02X} sprites={real_sprites:02X}",
            cpu.pc, ctx.attributes, ctx.routine, expected
        );
    }
}
