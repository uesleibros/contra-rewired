/// Captured inputs for one real `scuba_soldier_routine_01` call
/// (`$f14c`).
#[derive(Clone, Copy)]
pub struct ScubaSoldierRoutine01Ctx {
    pub x: usize,
    pub animation_delay: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub routine: u8,
}

pub fn verify_scuba_soldier_routine_01(
    ctx: ScubaSoldierRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::scuba_soldier::{scuba_soldier_routine_01, ScubaSoldierRoutine01Outcome};

    let x = ctx.x;
    let expected = scuba_soldier_routine_01(ctx.animation_delay, ctx.level_scrolling_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos, ctx.routine);
    *checked += 1;

    let real_sprite = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_animation_delay = bus.ram[0x538 + x];
    let real_attack_delay = bus.ram[0x558 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let base_ok = real_sprite == expected.sprite && real_sprite_attr == expected.sprite_attr && real_x_pos == expected.scroll.x_pos && real_y_pos == expected.scroll.y_pos;

    let mismatch = !base_ok
        || match expected.outcome {
            ScubaSoldierRoutine01Outcome::Waiting { animation_delay } | ScubaSoldierRoutine01Outcome::NotYetHighEnough { animation_delay } => real_animation_delay != animation_delay,
            ScubaSoldierRoutine01Outcome::Activated { attack_delay, delayed_routine } => {
                real_attack_delay != attack_delay || real_animation_delay != delayed_routine.animation_delay || real_routine != delayed_routine.routine_update.routine
            }
        };

    if mismatch {
        eprintln!(
            "MISMATCH(scuba_soldier_routine_01) frame={frame} pc={:04X} in=(delay={:02X} x={:02X} y={:02X} routine={:02X}): expected {:?}, got sprite={real_sprite:02X} sprite_attr={real_sprite_attr:02X} x={real_x_pos:02X} y={real_y_pos:02X} animation_delay={real_animation_delay:02X} attack_delay={real_attack_delay:02X} routine={real_routine:02X}",
            cpu.pc, ctx.animation_delay, ctx.x_pos, ctx.y_pos, ctx.routine, expected
        );
    }
}
