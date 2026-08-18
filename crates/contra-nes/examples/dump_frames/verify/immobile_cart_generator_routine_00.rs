/// Captured inputs for one real `immobile_cart_generator_routine_00`
/// call (`$b1e5`).
#[derive(Clone, Copy)]
pub struct ImmobileCartGeneratorRoutine00Ctx {
    pub x: usize,
    pub routine: u8,
}

pub fn verify_immobile_cart_generator_routine_00(
    ctx: ImmobileCartGeneratorRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::mine_cart::immobile_cart_generator_routine_00;

    let x = ctx.x;
    let expected = immobile_cart_generator_routine_00(ctx.routine);
    *checked += 1;

    let real_x_vel_fract = bus.ram[0x518 + x];
    let real_sprite = bus.ram[0x30A + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch =
        real_x_vel_fract != expected.init.x_vel_fract || real_sprite != expected.init.sprite || real_y_pos != expected.init.y_pos || real_routine != expected.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(immobile_cart_generator_routine_00) frame={frame} pc={:04X} in=(routine={:02X}): expected {:?}, got x_vel_fract={real_x_vel_fract:02X} sprite={real_sprite:02X} y={real_y_pos:02X} routine={real_routine:02X}",
            cpu.pc, ctx.routine, expected
        );
    }
}
