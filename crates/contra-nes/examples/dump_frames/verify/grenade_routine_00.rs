/// Captured inputs for one real `grenade_routine_00` call (`$8fd5`).
#[derive(Clone, Copy)]
pub struct GrenadeRoutine00Ctx {
    pub x: usize,
    pub y_pos: u8,
    pub routine: u8,
}

pub fn verify_grenade_routine_00(
    ctx: GrenadeRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::grenade::grenade_routine_00;

    let x = ctx.x;
    let expected = grenade_routine_00(ctx.y_pos, ctx.routine);
    *checked += 1;

    let real_var_1 = bus.ram[0x5B8 + x];
    let real_var_4 = bus.ram[0x5E8 + x];
    let real_attack_delay = bus.ram[0x558 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_var_1 != expected.var_1 || real_var_4 != expected.var_4 || real_attack_delay != expected.attack_delay || real_routine != expected.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(grenade_routine_00) frame={frame} pc={:04X} in=(y_pos={:02X} routine={:02X}): expected {:?}, got var_1={real_var_1:02X} var_4={real_var_4:02X} attack_delay={real_attack_delay:02X} routine={real_routine:02X}",
            cpu.pc, ctx.y_pos, ctx.routine, expected
        );
    }
}
