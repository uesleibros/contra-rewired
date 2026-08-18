/// Captured inputs for one real `mortar_shot_routine_00` call (`$f1d6`).
#[derive(Clone, Copy)]
pub struct MortarShotRoutine00Ctx {
    pub x: usize,
    pub enemy_attributes: u8,
    pub enemy_var_1: u8,
    pub routine: u8,
}

pub fn verify_mortar_shot_routine_00(
    ctx: MortarShotRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::mortar_shot::mortar_shot_routine_00;

    let x = ctx.x;
    let expected = mortar_shot_routine_00(ctx.enemy_attributes, ctx.enemy_var_1, ctx.routine);
    *checked += 1;

    let real_state_width = bus.ram[0x598 + x];
    let real_sprite = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_y_vel_fract = bus.ram[0x4F8 + x];
    let real_y_vel_fast = bus.ram[0x4E8 + x];
    let real_x_vel_fract = bus.ram[0x518 + x];
    let real_x_vel_fast = bus.ram[0x508 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_state_width != expected.state_width
        || real_sprite != expected.sprite
        || real_sprite_attr != expected.sprite_attr
        || real_y_vel_fract != expected.y_vel_fract
        || real_y_vel_fast != expected.y_vel_fast
        || real_x_vel_fract != expected.x_vel_fract
        || real_x_vel_fast != expected.x_vel_fast
        || real_routine != expected.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(mortar_shot_routine_00) frame={frame} pc={:04X} in=(attrs={:02X} var_1={:02X} routine={:02X}): expected {:?}, got state_width={real_state_width:02X} sprite={real_sprite:02X} sprite_attr={real_sprite_attr:02X} y_fract={real_y_vel_fract:02X} y_fast={real_y_vel_fast:02X} x_fract={real_x_vel_fract:02X} x_fast={real_x_vel_fast:02X} routine={real_routine:02X}",
            cpu.pc, ctx.enemy_attributes, ctx.enemy_var_1, ctx.routine, expected
        );
    }
}
