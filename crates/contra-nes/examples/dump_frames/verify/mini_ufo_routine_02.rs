/// Captured inputs for one real `mini_ufo_routine_02` call (`$a922`).
#[derive(Clone, Copy)]
pub struct MiniUfoRoutine02Ctx {
    pub x: usize,
    pub animation_delay: u8,
    pub sprite: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub y_vel_accum: u8,
    pub y_vel_fract: u8,
    pub y_vel_fast: u8,
    pub frame_scroll: u8,
    pub routine: u8,
}

pub fn verify_mini_ufo_routine_02(
    ctx: MiniUfoRoutine02Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::ufo::{mini_ufo_routine_02, MiniUfoRoutine02Outcome};

    let x = ctx.x;
    let expected = mini_ufo_routine_02(ctx.animation_delay, ctx.sprite, ctx.x_pos, ctx.y_pos, ctx.y_vel_accum, ctx.y_vel_fract, ctx.y_vel_fast, ctx.frame_scroll, ctx.routine);
    *checked += 1;

    let real_animation_delay = bus.ram[0x538 + x];
    let real_sprite = bus.ram[0x30A + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_y_vel_accum = bus.ram[0x4C8 + x];
    let real_x_vel_fast = bus.ram[0x508 + x];
    let real_x_vel_fract = bus.ram[0x518 + x];
    let real_y_vel_fract = bus.ram[0x4F8 + x];
    let real_y_vel_fast = bus.ram[0x4E8 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let base_ok = real_animation_delay == expected.animation_delay
        && real_y_pos == expected.y.pos
        && real_y_vel_accum == expected.y.vel_accum
        && match expected.sprite {
            Some(s) => real_sprite == s,
            None => real_sprite == ctx.sprite,
        };

    let outcome_ok = match expected.outcome {
        MiniUfoRoutine02Outcome::StillDescending => true,
        MiniUfoRoutine02Outcome::ReachedBottom { y_pos, x_vel_fast, x_vel_fract, routine_update, .. } => {
            real_y_pos == y_pos && real_x_vel_fast == x_vel_fast && real_x_vel_fract == x_vel_fract && real_y_vel_fract == 0 && real_y_vel_fast == 0 && real_routine == routine_update.routine
        }
    };

    let mismatch = !base_ok || !outcome_ok;

    if mismatch {
        eprintln!(
            "MISMATCH(mini_ufo_routine_02) frame={frame} pc={:04X} in=(delay={:02X} x={:02X} y={:02X} routine={:02X}): expected {:?}, got animation_delay={real_animation_delay:02X} sprite={real_sprite:02X} y={real_y_pos:02X} x_vel_fast={real_x_vel_fast:02X} x_vel_fract={real_x_vel_fract:02X} routine={real_routine:02X}",
            cpu.pc, ctx.animation_delay, ctx.x_pos, ctx.y_pos, ctx.routine, expected
        );
    }
}
