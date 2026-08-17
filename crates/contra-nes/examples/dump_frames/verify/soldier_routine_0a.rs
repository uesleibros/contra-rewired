/// Captured inputs for one real `soldier_routine_0a` call (`$88a1`).
#[derive(Clone, Copy)]
pub struct SoldierRoutine0aCtx {
    pub x: usize,
    pub animation_delay: u8,
    pub frame: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub var_2: u8,
    pub var_1: u8,
    pub scroll_type: u8,
    pub frame_scroll: u8,
    pub routine: u8,
}

pub fn verify_soldier_routine_0a(ctx: SoldierRoutine0aCtx, cpu: &contra_nes::cpu::Cpu, bus: &contra_nes::bus::NesBus, frame: u32, checked: &mut u64) {
    use contra_native::enemy::soldier::{soldier_routine_0a, SoldierRoutine0aOutcome};

    let x = ctx.x;
    let expected =
        soldier_routine_0a(ctx.animation_delay, ctx.frame, ctx.x_pos, ctx.y_pos, ctx.var_2, ctx.var_1, ctx.scroll_type, ctx.frame_scroll);
    *checked += 1;

    let real_delay = bus.ram[0x538 + x];
    let real_frame = bus.ram[0x568 + x];
    let real_var_1 = bus.ram[0x5B8 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_sprites = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = match expected {
        SoldierRoutine0aOutcome::Waiting(tail) => {
            let removed = tail.scroll.should_remove;
            let expected_sprites = if removed { 0 } else { tail.sprite.sprite };
            let expected_routine = if removed { 0 } else { ctx.routine };
            real_frame != ctx.frame
                || real_var_1 != tail.sprite.var_1
                || real_x_pos != tail.scroll.x_pos
                || real_y_pos != tail.scroll.y_pos
                || real_sprites != expected_sprites
                || real_sprite_attr != tail.sprite.sprite_attr
                || real_delay != ctx.animation_delay.wrapping_sub(1)
                || real_routine != expected_routine
        }
        SoldierRoutine0aOutcome::Removed(_) => real_routine != 0 || real_sprites != 0,
        SoldierRoutine0aOutcome::StillSplashing { animation_delay, enemy_frame, tail } => {
            let removed = tail.scroll.should_remove;
            let expected_sprites = if removed { 0 } else { tail.sprite.sprite };
            let expected_routine = if removed { 0 } else { ctx.routine };
            real_frame != enemy_frame
                || real_var_1 != tail.sprite.var_1
                || real_x_pos != tail.scroll.x_pos
                || real_y_pos != tail.scroll.y_pos
                || real_sprites != expected_sprites
                || real_sprite_attr != tail.sprite.sprite_attr
                || real_delay != animation_delay
                || real_routine != expected_routine
        }
    };

    if mismatch {
        eprintln!(
            "MISMATCH(soldier_routine_0a) frame={frame} pc={:04X} in=(delay={:02X} frame={:02X} x={:02X} y={:02X} var_2={:02X} var_1={:02X} scroll_type={:02X} frame_scroll={:02X} routine={:02X}): expected {:?}, got delay={real_delay:02X} frame={real_frame:02X} var_1={real_var_1:02X} x={real_x_pos:02X} y={real_y_pos:02X} sprites={real_sprites:02X} sprite_attr={real_sprite_attr:02X} routine={real_routine:02X}",
            cpu.pc, ctx.animation_delay, ctx.frame, ctx.x_pos, ctx.y_pos, ctx.var_2, ctx.var_1, ctx.scroll_type, ctx.frame_scroll, ctx.routine, expected
        );
    }
}

