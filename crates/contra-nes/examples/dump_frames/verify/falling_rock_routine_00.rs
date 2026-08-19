/// Captured inputs for one real `falling_rock_routine_00` call
/// (`$9889`).
#[derive(Clone, Copy)]
pub struct FallingRockRoutine00Ctx {
    pub x: usize,
    pub routine: u8,
}

pub fn verify_falling_rock_routine_00(
    ctx: FallingRockRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::rock::falling_rock_routine_00;

    let x = ctx.x;
    let expected = falling_rock_routine_00(ctx.routine);
    *checked += 1;

    let real_animation_delay = bus.ram[0x538 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_animation_delay != expected.animation_delay || real_routine != expected.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(falling_rock_routine_00) frame={frame} pc={:04X} in=(routine={:02X}): expected {:?}, got animation_delay={real_animation_delay:02X} routine={real_routine:02X}",
            cpu.pc, ctx.routine, expected
        );
    }
}
