/// Captured inputs for one real `grenade_launcher_routine_00` call
/// (`$9468`).
#[derive(Clone, Copy)]
pub struct GrenadeLauncherRoutine00Ctx {
    pub x: usize,
    pub attributes: u8,
    pub x_pos: u8,
    pub sprite_x_pos: [u8; 2],
    pub player_state: [u8; 2],
    pub routine: u8,
}

pub fn verify_grenade_launcher_routine_00(
    ctx: GrenadeLauncherRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::grenade_launcher::grenade_launcher_routine_00;

    let x = ctx.x;
    let expected = grenade_launcher_routine_00(ctx.attributes, ctx.x_pos, ctx.sprite_x_pos, ctx.player_state, ctx.routine);
    *checked += 1;

    let real_flag = bus.ram[0x8A];
    let real_var_2 = bus.ram[0x5C8 + x];
    let real_x_fract = bus.ram[0x518 + x];
    let real_x_fast = bus.ram[0x508 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_animation_delay = bus.ram[0x538 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_flag != expected.grenade_launcher_flag
        || real_var_2 != expected.var_2
        || real_x_fract != expected.init.x_velocity.0
        || real_x_fast != expected.init.x_velocity.1
        || real_x_pos != expected.init.x_pos
        || real_y_pos != expected.init.y_pos
        || real_animation_delay != expected.delayed_routine.animation_delay
        || real_routine != expected.delayed_routine.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(grenade_launcher_routine_00) frame={frame} pc={:04X} in=(attrs={:02X} x={:02X} routine={:02X}): expected {:?}, got flag={real_flag:02X} var_2={real_var_2:02X} xvel=({real_x_fract:02X},{real_x_fast:02X}) x={real_x_pos:02X} y={real_y_pos:02X} animation_delay={real_animation_delay:02X} routine={real_routine:02X}",
            cpu.pc, ctx.attributes, ctx.x_pos, ctx.routine, expected
        );
    }
}
