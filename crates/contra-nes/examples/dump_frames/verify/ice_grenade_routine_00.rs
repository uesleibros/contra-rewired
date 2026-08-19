/// Captured inputs for one real `ice_grenade_routine_00` call (`$a3b5`).
#[derive(Clone, Copy)]
pub struct IceGrenadeRoutine00Ctx {
    pub x: usize,
    pub routine: u8,
}

pub fn verify_ice_grenade_routine_00(
    ctx: IceGrenadeRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::ice::ice_grenade_routine_00;

    let x = ctx.x;
    let expected = ice_grenade_routine_00(ctx.routine);
    *checked += 1;

    let real_sprite_attr = bus.ram[0x358 + x];
    let real_x_vel_fract = bus.ram[0x518 + x];
    let real_x_vel_fast = bus.ram[0x508 + x];
    let real_y_vel_fract = bus.ram[0x4F8 + x];
    let real_y_vel_fast = bus.ram[0x4E8 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_sprite_attr != expected.sprite_attr
        || real_x_vel_fract != expected.x_vel_fract
        || real_x_vel_fast != expected.x_vel_fast
        || real_y_vel_fract != expected.y_vel_fract
        || real_y_vel_fast != expected.y_vel_fast
        || real_routine != expected.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(ice_grenade_routine_00) frame={frame} pc={:04X} in=(routine={:02X}): expected {:?}, got sprite_attr={real_sprite_attr:02X} x_vel_fract={real_x_vel_fract:02X} x_vel_fast={real_x_vel_fast:02X} y_vel_fract={real_y_vel_fract:02X} y_vel_fast={real_y_vel_fast:02X} routine={real_routine:02X}",
            cpu.pc, ctx.routine, expected
        );
    }
}
