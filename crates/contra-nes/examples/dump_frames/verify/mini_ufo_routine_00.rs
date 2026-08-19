/// Captured inputs for one real `mini_ufo_routine_00` call (`$a8fa`).
#[derive(Clone, Copy)]
pub struct MiniUfoRoutine00Ctx {
    pub x: usize,
    pub animation_delay: u8,
    pub sprite: u8,
    pub routine: u8,
}

pub fn verify_mini_ufo_routine_00(
    ctx: MiniUfoRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::ufo::{mini_ufo_routine_00, MiniUfoRoutine00Outcome};

    let x = ctx.x;
    let expected = mini_ufo_routine_00(ctx.animation_delay, ctx.sprite, ctx.routine);
    *checked += 1;

    let real_animation_delay = bus.ram[0x538 + x];
    let real_sprite = bus.ram[0x30A + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_animation_delay != expected.animation_delay
        || match expected.outcome {
            MiniUfoRoutine00Outcome::Advanced(update) => real_routine != update.routine,
            MiniUfoRoutine00Outcome::Animating { sprite } => match sprite {
                Some(s) => real_sprite != s,
                None => real_sprite != ctx.sprite,
            },
        };

    if mismatch {
        eprintln!(
            "MISMATCH(mini_ufo_routine_00) frame={frame} pc={:04X} in=(delay={:02X} sprite={:02X} routine={:02X}): expected {:?}, got animation_delay={real_animation_delay:02X} sprite={real_sprite:02X} routine={real_routine:02X}",
            cpu.pc, ctx.animation_delay, ctx.sprite, ctx.routine, expected
        );
    }
}
