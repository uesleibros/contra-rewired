/// Captured inputs for one real `ice_separator_routine_00` call
/// (`$a985`).
#[derive(Clone, Copy)]
pub struct IceSeparatorRoutine00Ctx {
    pub x: usize,
    pub tank_ice_joint_scroll_flag: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
}

pub fn verify_ice_separator_routine_00(
    ctx: IceSeparatorRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::ice::{ice_separator_routine_00, IceSeparatorRoutine00Outcome};

    let x = ctx.x;
    let expected = ice_separator_routine_00(ctx.tank_ice_joint_scroll_flag, ctx.level_scrolling_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos);
    *checked += 1;

    let real_sprite = bus.ram[0x30A + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];

    let outcome_ok = match expected.outcome {
        IceSeparatorRoutine00Outcome::Scrolled(scroll) => real_x_pos == scroll.x_pos && real_y_pos == scroll.y_pos,
        IceSeparatorRoutine00Outcome::NoScrollThisFrame => real_x_pos == ctx.x_pos,
        IceSeparatorRoutine00Outcome::Nudged { x_pos } => real_x_pos == x_pos,
    };

    let mismatch = real_sprite != expected.sprite || !outcome_ok;

    if mismatch {
        eprintln!(
            "MISMATCH(ice_separator_routine_00) frame={frame} pc={:04X} in=(flag={:02X} x={:02X} y={:02X}): expected {:?}, got sprite={real_sprite:02X} x={real_x_pos:02X} y={real_y_pos:02X}",
            cpu.pc, ctx.tank_ice_joint_scroll_flag, ctx.x_pos, ctx.y_pos, expected
        );
    }
}
