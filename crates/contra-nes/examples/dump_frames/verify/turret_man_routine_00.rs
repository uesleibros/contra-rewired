/// Captured inputs for one real `turret_man_routine_00` call (`$f0c9`).
#[derive(Clone, Copy)]
pub struct TurretManRoutine00Ctx {
    pub x: usize,
    pub enemy_attributes: u8,
    pub routine: u8,
}

pub fn verify_turret_man_routine_00(
    ctx: TurretManRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::turret_man::turret_man_routine_00;

    let x = ctx.x;
    let expected = turret_man_routine_00(ctx.enemy_attributes, ctx.routine);
    *checked += 1;

    let real_sprite = bus.ram[0x30A + x];
    let real_animation_delay = bus.ram[0x538 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_sprite != expected.sprite
        || real_animation_delay != expected.delayed_routine.animation_delay
        || real_routine != expected.delayed_routine.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(turret_man_routine_00) frame={frame} pc={:04X} in=(attributes={:02X} routine={:02X}): expected {:?}, got sprite={real_sprite:02X} animation_delay={real_animation_delay:02X} routine={real_routine:02X}",
            cpu.pc, ctx.enemy_attributes, ctx.routine, expected
        );
    }
}
