/// Captured inputs for one real `turret_man_bullet_routine_00` call
/// (`$f11f`).
#[derive(Clone, Copy)]
pub struct TurretManBulletRoutine00Ctx {
    pub x: usize,
    pub routine: u8,
}

pub fn verify_turret_man_bullet_routine_00(
    ctx: TurretManBulletRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::turret_man::turret_man_bullet_routine_00;

    let x = ctx.x;
    let expected = turret_man_bullet_routine_00(ctx.routine);
    *checked += 1;

    let real_x_vel_fast = bus.ram[0x508 + x];
    let real_x_vel_fract = bus.ram[0x518 + x];
    let real_sprite = bus.ram[0x30A + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_x_vel_fast != expected.x_vel_fast || real_x_vel_fract != expected.x_vel_fract || real_sprite != expected.sprite || real_routine != expected.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(turret_man_bullet_routine_00) frame={frame} pc={:04X} in=(routine={:02X}): expected {:?}, got x_vel_fast={real_x_vel_fast:02X} x_vel_fract={real_x_vel_fract:02X} sprite={real_sprite:02X} routine={real_routine:02X}",
            cpu.pc, ctx.routine, expected
        );
    }
}
