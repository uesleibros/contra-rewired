/// Captured inputs for one real `soldier_routine_05` call (`$8900`).
#[derive(Clone, Copy)]
pub struct SoldierRoutine05Ctx {
    pub x: usize,
    pub frame: u8,
    pub var_2: u8,
    pub var_1: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub y_fract: u8,
    pub y_fast: u8,
    pub x_accum: u8,
    pub x_fract: u8,
    pub x_fast: u8,
    pub y_accum: u8,
    pub scroll_type: u8,
    pub frame_scroll: u8,
    pub animation_delay: u8,
    pub routine: u8,
}

pub fn verify_soldier_routine_05(ctx: SoldierRoutine05Ctx, cpu: &contra_nes::cpu::Cpu, bus: &contra_nes::bus::NesBus, frame: u32, checked: &mut u64) {
    use contra_native::enemy::soldier::{soldier_routine_05, SoldierRoutine05Outcome};

    let x = ctx.x;
    let expected = soldier_routine_05(
        ctx.frame, ctx.var_2, ctx.var_1, ctx.x_pos, ctx.y_pos, ctx.y_fract, ctx.y_fast, ctx.x_accum, ctx.x_fract, ctx.x_fast, ctx.y_accum,
        ctx.scroll_type, ctx.frame_scroll, ctx.animation_delay, ctx.routine,
    );
    *checked += 1;

    let real_sprites = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_y_fract = bus.ram[0x4F8 + x];
    let real_y_fast = bus.ram[0x4E8 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_delay = bus.ram[0x538 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let sprite_ok = real_sprites == expected.sprite.sprite && real_sprite_attr == expected.sprite.sprite_attr;
    let y_vel_ok = real_y_fract == expected.y_velocity.0 && real_y_fast == expected.y_velocity.1;

    let mismatch = match expected.outcome {
        SoldierRoutine05Outcome::OffTopAdvance(routine_update) => {
            !sprite_ok
                || !y_vel_ok
                || real_x_pos != ctx.x_pos
                || real_y_pos != ctx.y_pos
                || real_delay != ctx.animation_delay
                || real_routine != routine_update.routine
        }
        SoldierRoutine05Outcome::StillWaiting { position, animation_delay } => {
            let removed = position.removed.is_some();
            let expected_routine = if removed { 0 } else { ctx.routine };
            !sprite_ok
                || !y_vel_ok
                || real_x_pos != position.x.pos
                || real_y_pos != position.y.pos
                || real_delay != animation_delay
                || real_routine != expected_routine
        }
        SoldierRoutine05Outcome::Advanced { position, animation_delay, routine_update } => {
            let removed = position.removed.is_some();
            let expected_routine = if removed { 0 } else { routine_update.routine };
            !sprite_ok
                || !y_vel_ok
                || real_x_pos != position.x.pos
                || real_y_pos != position.y.pos
                || real_delay != animation_delay
                || real_routine != expected_routine
        }
    };

    if mismatch {
        eprintln!(
            "MISMATCH(soldier_routine_05) frame={frame} pc={:04X} in=(frame={:02X} var_2={:02X} var_1={:02X} x={:02X} y={:02X} yvel=({:02X},{:02X}) scroll_type={:02X} frame_scroll={:02X} delay={:02X} routine={:02X}): expected {:?}, got sprites={real_sprites:02X} sprite_attr={real_sprite_attr:02X} yvel=({real_y_fract:02X},{real_y_fast:02X}) x={real_x_pos:02X} y={real_y_pos:02X} delay={real_delay:02X} routine={real_routine:02X}",
            cpu.pc, ctx.frame, ctx.var_2, ctx.var_1, ctx.x_pos, ctx.y_pos, ctx.y_fract, ctx.y_fast, ctx.scroll_type, ctx.frame_scroll, ctx.animation_delay, ctx.routine, expected
        );
    }
}

