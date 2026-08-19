/// Captured inputs for one real `boss_ufo_bomb_routine_00` call
/// (`$a974`).
#[derive(Clone, Copy)]
pub struct BossUfoBombRoutine00Ctx {
    pub x: usize,
    pub y_pos: u8,
    pub y_vel_fract: u8,
    pub y_vel_fast: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub x_vel_accum: u8,
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    pub y_vel_accum: u8,
    pub routine: u8,
}

pub fn verify_boss_ufo_bomb_routine_00(
    ctx: BossUfoBombRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::ufo::{boss_ufo_bomb_routine_00, BossUfoBombRoutine00Outcome};

    let x = ctx.x;
    let (expected_y_vel_fract, expected_y_vel_fast, expected_outcome) = boss_ufo_bomb_routine_00(
        ctx.y_pos,
        ctx.y_vel_fract,
        ctx.y_vel_fast,
        ctx.level_scrolling_type,
        ctx.frame_scroll,
        ctx.x_pos,
        ctx.x_vel_accum,
        ctx.x_vel_fract,
        ctx.x_vel_fast,
        ctx.y_vel_accum,
        ctx.routine,
    );
    *checked += 1;

    let real_y_vel_fract = bus.ram[0x4F8 + x];
    let real_y_vel_fast = bus.ram[0x4E8 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_x_vel_accum = bus.ram[0x4D8 + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_y_vel_accum = bus.ram[0x4C8 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let velocity_ok = real_y_vel_fract == expected_y_vel_fract && real_y_vel_fast == expected_y_vel_fast;

    let outcome_ok = match &expected_outcome {
        BossUfoBombRoutine00Outcome::Falling(position) => {
            real_x_pos == position.x.pos && real_x_vel_accum == position.x.vel_accum && real_y_pos == position.y.pos && real_y_vel_accum == position.y.vel_accum
        }
        BossUfoBombRoutine00Outcome::Exploding(update) => real_routine == update.routine,
    };

    let mismatch = !velocity_ok || !outcome_ok;

    if mismatch {
        eprintln!(
            "MISMATCH(boss_ufo_bomb_routine_00) frame={frame} pc={:04X} in=(y={:02X} routine={:02X}): expected y_vel_fract={expected_y_vel_fract:02X} y_vel_fast={expected_y_vel_fast:02X} outcome={expected_outcome:?}, got y_vel_fract={real_y_vel_fract:02X} y_vel_fast={real_y_vel_fast:02X} x={real_x_pos:02X} y={real_y_pos:02X} routine={real_routine:02X}",
            cpu.pc, ctx.y_pos, ctx.routine
        );
    }
}
