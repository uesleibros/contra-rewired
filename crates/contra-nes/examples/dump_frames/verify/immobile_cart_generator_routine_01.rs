/// Captured inputs for one real `immobile_cart_generator_routine_01`
/// call (`$b1fb`). Only the `Advanced` outcome (real ASM: `bne cart_
/// advance_enemy_routine`) is live-verified - the `ScrollOnly` outcome
/// falls straight through into `rising_spiked_wall_routine_03`'s own
/// entry ($b200), a real, unrelated enemy's own routine-table target
/// too, so hooking that address here would also catch genuine rising-
/// spiked-wall calls and misattribute them.
#[derive(Clone, Copy)]
pub struct ImmobileCartGeneratorRoutine01Ctx {
    pub x: usize,
    pub enemy_frame: u8,
    pub routine: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
}

pub fn verify_immobile_cart_generator_routine_01(
    ctx: ImmobileCartGeneratorRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::mine_cart::{immobile_cart_generator_routine_01, ImmobileCartGeneratorRoutine01Outcome};

    let x = ctx.x;
    let expected = immobile_cart_generator_routine_01(ctx.enemy_frame, ctx.routine, ctx.level_scrolling_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos);
    *checked += 1;

    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = match expected {
        ImmobileCartGeneratorRoutine01Outcome::Advanced(update) => real_routine != update.routine,
        ImmobileCartGeneratorRoutine01Outcome::ScrollOnly(_) => false,
    };

    if mismatch {
        eprintln!(
            "MISMATCH(immobile_cart_generator_routine_01) frame={frame} pc={:04X} in=(frame={:02X} routine={:02X}): expected {:?}, got routine={real_routine:02X}",
            cpu.pc, ctx.enemy_frame, ctx.routine, expected
        );
    }
}
