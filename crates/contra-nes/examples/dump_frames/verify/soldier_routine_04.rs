/// Captured inputs for one real `soldier_routine_04` call (`$88c3`).
#[derive(Clone, Copy)]
pub struct SoldierRoutine04Ctx {
    pub x: usize,
    pub x_pos: u8,
    pub y_pos: u8,
    pub var_2: u8,
    pub var_1: u8,
    pub state_width: u8,
    pub scroll_type: u8,
    pub frame_scroll: u8,
    pub routine: u8,
}

pub fn verify_soldier_routine_04(ctx: SoldierRoutine04Ctx, cpu: &contra_nes::cpu::Cpu, bus: &contra_nes::bus::NesBus, frame: u32, checked: &mut u64) {
    use contra_native::enemy::soldier::soldier_routine_04;

    let x = ctx.x;
    let expected = soldier_routine_04(ctx.x_pos, ctx.y_pos, ctx.var_2, ctx.var_1, ctx.state_width, ctx.scroll_type, ctx.frame_scroll, ctx.routine);
    *checked += 1;

    let real_frame = bus.ram[0x568 + x];
    let real_state_width = bus.ram[0x598 + x];
    let real_y_fract = bus.ram[0x4F8 + x];
    let real_y_fast = bus.ram[0x4E8 + x];
    let real_x_fract = bus.ram[0x518 + x];
    let real_x_fast = bus.ram[0x508 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_sprites = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_delay = bus.ram[0x538 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let removed = expected.scroll.should_remove;
    let expected_sprites = if removed { 0 } else { expected.sprite.sprite };
    let expected_routine = if removed { 0 } else { expected.delayed_routine.routine_update.routine };

    let mismatch = real_frame != 0x0B
        || real_state_width != expected.state_width
        || real_y_fract != expected.y_velocity.0
        || real_y_fast != expected.y_velocity.1
        || real_x_fract != expected.x_velocity.0
        || real_x_fast != expected.x_velocity.1
        || real_x_pos != expected.scroll.x_pos
        || real_y_pos != expected.scroll.y_pos
        || real_sprites != expected_sprites
        || real_sprite_attr != expected.sprite.sprite_attr
        || real_delay != expected.delayed_routine.animation_delay
        || real_routine != expected_routine;

    if mismatch {
        eprintln!(
            "MISMATCH(soldier_routine_04) frame={frame} pc={:04X} in=(x={:02X} y={:02X} var_2={:02X} var_1={:02X} state_width={:02X} scroll_type={:02X} frame_scroll={:02X} routine={:02X}): expected {:?}, got frame={real_frame:02X} state_width={real_state_width:02X} yvel=({real_y_fract:02X},{real_y_fast:02X}) xvel=({real_x_fract:02X},{real_x_fast:02X}) x={real_x_pos:02X} y={real_y_pos:02X} sprites={real_sprites:02X} sprite_attr={real_sprite_attr:02X} delay={real_delay:02X} routine={real_routine:02X}",
            cpu.pc, ctx.x_pos, ctx.y_pos, ctx.var_2, ctx.var_1, ctx.state_width, ctx.scroll_type, ctx.frame_scroll, ctx.routine, expected
        );
    }
}

