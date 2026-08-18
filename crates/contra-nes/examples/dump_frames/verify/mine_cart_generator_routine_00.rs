/// Captured inputs for one real `mine_cart_generator_routine_00` call
/// (`$b122`).
#[derive(Clone, Copy)]
pub struct MineCartGeneratorRoutine00Ctx {
    pub x: usize,
    pub routine: u8,
}

pub fn verify_mine_cart_generator_routine_00(
    ctx: MineCartGeneratorRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::mine_cart::mine_cart_generator_routine_00;

    let x = ctx.x;
    let expected = mine_cart_generator_routine_00(ctx.routine);
    *checked += 1;

    let real_frame = bus.ram[0x568 + x];
    let real_animation_delay = bus.ram[0x538 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_frame != expected.frame || real_animation_delay != expected.delayed_routine.animation_delay || real_routine != expected.delayed_routine.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(mine_cart_generator_routine_00) frame={frame} pc={:04X} in=(routine={:02X}): expected {:?}, got frame={real_frame:02X} animation_delay={real_animation_delay:02X} routine={real_routine:02X}",
            cpu.pc, ctx.routine, expected
        );
    }
}
