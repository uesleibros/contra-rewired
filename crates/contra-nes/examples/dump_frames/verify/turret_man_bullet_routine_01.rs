/// Captured inputs for one real `turret_man_bullet_routine_01` call
/// (`$f131`).
#[derive(Clone, Copy)]
pub struct TurretManBulletRoutine01Ctx {
    pub x: usize,
    pub x_pos: u8,
    pub routine: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_vel_accum: u8,
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    pub y_pos: u8,
    pub y_vel_accum: u8,
    pub y_vel_fract: u8,
    pub y_vel_fast: u8,
}

pub fn verify_turret_man_bullet_routine_01(
    ctx: TurretManBulletRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::turret_man::{turret_man_bullet_routine_01, TurretManBulletRoutine01Outcome};

    let x = ctx.x;
    let expected = turret_man_bullet_routine_01(
        ctx.x_pos,
        ctx.routine,
        ctx.level_scrolling_type,
        ctx.frame_scroll,
        ctx.x_vel_accum,
        ctx.x_vel_fract,
        ctx.x_vel_fast,
        ctx.y_pos,
        ctx.y_vel_accum,
        ctx.y_vel_fract,
        ctx.y_vel_fast,
    );
    *checked += 1;

    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = match expected {
        TurretManBulletRoutine01Outcome::Advanced(update) => real_routine != update.routine,
        TurretManBulletRoutine01Outcome::Position(position) => {
            let real_x_pos = bus.ram[0x33E + x];
            let real_x_vel_accum = bus.ram[0x4D8 + x];
            let real_y_pos = bus.ram[0x324 + x];
            let real_y_vel_accum = bus.ram[0x4C8 + x];
            real_x_pos != position.x.pos || real_x_vel_accum != position.x.vel_accum || real_y_pos != position.y.pos || real_y_vel_accum != position.y.vel_accum
        }
    };

    if mismatch {
        eprintln!(
            "MISMATCH(turret_man_bullet_routine_01) frame={frame} pc={:04X} in=(x_pos={:02X} routine={:02X}): expected {:?}, got routine={real_routine:02X}",
            cpu.pc, ctx.x_pos, ctx.routine, expected
        );
    }
}
