/// Captured inputs for one real `weapon_item_routine_00` call (`$8007`).
#[derive(Clone, Copy)]
pub struct WeaponItemRoutine00Ctx {
    pub x: usize,
    pub level_location_type: u8,
    pub y_pos: u8,
    pub x_pos: u8,
    pub level_scrolling_type: u8,
    pub routine: u8,
}

pub fn verify_weapon_item_routine_00(
    ctx: WeaponItemRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::weapon_item::{weapon_item_routine_00, WeaponItemRoutine00Outcome};

    let x = ctx.x;
    let expected = weapon_item_routine_00(ctx.level_location_type, ctx.y_pos, ctx.x_pos, ctx.level_scrolling_type, ctx.routine);
    *checked += 1;

    let real_state_width = bus.ram[0x598 + x];
    let real_score_collision = bus.ram[0x588 + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mut mismatch = real_state_width != expected.state_width || real_score_collision != expected.score_collision || real_sprite_attr != expected.sprite_attr;

    match expected.outcome {
        WeaponItemRoutine00Outcome::Indoor { var_1, velocity, var_4, var_b, routine_update } => {
            let real_var_1 = bus.ram[0x5B8 + x];
            let real_var_4 = bus.ram[0x5E8 + x];
            let real_var_b = bus.ram[0x558 + x];
            let real_x_fract = bus.ram[0x518 + x];
            let real_x_fast = bus.ram[0x508 + x];
            let real_y_fract = bus.ram[0x4F8 + x];
            let real_y_fast = bus.ram[0x4E8 + x];
            mismatch = mismatch
                || real_var_1 != var_1
                || real_var_4 != var_4
                || real_var_b != var_b
                || (real_x_fract, real_x_fast) != velocity.x_velocity
                || (real_y_fract, real_y_fast) != velocity.y_velocity
                || real_routine != routine_update.routine;
        }
        WeaponItemRoutine00Outcome::Outdoor { y_velocity, x_velocity, routine_update } => {
            let real_x_fract = bus.ram[0x518 + x];
            let real_x_fast = bus.ram[0x508 + x];
            let real_y_fract = bus.ram[0x4F8 + x];
            let real_y_fast = bus.ram[0x4E8 + x];
            mismatch = mismatch || (real_x_fract, real_x_fast) != x_velocity || (real_y_fract, real_y_fast) != y_velocity || real_routine != routine_update.routine;
        }
    }

    if mismatch {
        eprintln!(
            "MISMATCH(weapon_item_routine_00) frame={frame} pc={:04X} in=(loc_type={:02X} y={:02X} x={:02X} scroll_type={:02X} routine={:02X}): expected {:?}, got state_width={real_state_width:02X} score_collision={real_score_collision:02X} sprite_attr={real_sprite_attr:02X} routine={real_routine:02X}",
            cpu.pc, ctx.level_location_type, ctx.y_pos, ctx.x_pos, ctx.level_scrolling_type, ctx.routine, expected
        );
    }
}
