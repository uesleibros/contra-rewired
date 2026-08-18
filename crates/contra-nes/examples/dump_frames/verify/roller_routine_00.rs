/// Captured inputs for one real `roller_routine_00` call (`$8f8c`).
#[derive(Clone, Copy)]
pub struct RollerRoutine00Ctx {
    pub x: usize,
    pub routine: u8,
}

pub fn verify_roller_routine_00(
    ctx: RollerRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::roller::roller_routine_00;

    let x = ctx.x;
    let expected = roller_routine_00(ctx.routine);
    *checked += 1;

    let real_y_pos = bus.ram[0x324 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_y_pos != expected.y_pos || real_routine != expected.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(roller_routine_00) frame={frame} pc={:04X} in=(routine={:02X}): expected {:?}, got y_pos={real_y_pos:02X} routine={real_routine:02X}",
            cpu.pc, ctx.routine, expected
        );
    }
}
