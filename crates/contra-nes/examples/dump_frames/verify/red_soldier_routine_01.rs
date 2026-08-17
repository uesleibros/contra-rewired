/// Captured inputs for one real `red_soldier_routine_01` call (`$a266`).
/// See `VERIFY_RED_SOLDIER_ROUTINE_01`'s comment in `main` for the real
/// exits, the bank gate, and the nested-return disambiguation this needs.
#[derive(Clone, Copy)]
pub struct RedSoldierRoutine01Ctx {
    pub x: usize,
    pub frame: u8,
    pub frame_counter: u8,
    pub attributes: u8,
    pub var_2: u8,
    pub scroll_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub x_accum: u8,
    pub x_fract: u8,
    pub x_fast: u8,
    pub y_pos: u8,
    pub y_accum: u8,
    pub y_fract: u8,
    pub y_fast: u8,
    pub sprite_x_pos: [u8; 2],
    pub player_state: [u8; 2],
    pub routine: u8,
}

pub fn verify_red_soldier_routine_01(
    ctx: RedSoldierRoutine01Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::red_blue_soldier::{red_soldier_routine_01, RedSoldierRoutine01Outcome};

    let x = ctx.x;
    let expected = red_soldier_routine_01(
        ctx.frame, ctx.frame_counter, ctx.attributes, ctx.var_2, ctx.scroll_type, ctx.frame_scroll, ctx.x_pos,
        ctx.x_accum, ctx.x_fract, ctx.x_fast, ctx.y_pos, ctx.y_accum, ctx.y_fract, ctx.y_fast, ctx.sprite_x_pos,
        ctx.player_state, ctx.routine,
    );
    *checked += 1;

    let real_frame = bus.ram[0x568 + x];
    let real_sprites = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_var_1 = bus.ram[0x5B8 + x];
    let real_attack_delay = bus.ram[0x558 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let removed = expected.position.removed.is_some();
    let expected_sprites = if removed { 0 } else { expected.sprites };

    let mismatch = real_frame != expected.enemy_frame
        || real_sprites != expected_sprites
        || real_sprite_attr != expected.sprite_attr
        || real_x_pos != expected.position.x.pos
        || real_y_pos != expected.position.y.pos
        || match expected.outcome {
            RedSoldierRoutine01Outcome::AlreadyFired | RedSoldierRoutine01Outcome::StillRunning => {
                real_routine != (if removed { 0 } else { ctx.routine })
            }
            RedSoldierRoutine01Outcome::Attack { var_1, attack_delay, routine_update } => {
                real_var_1 != var_1
                    || real_attack_delay != attack_delay
                    || real_routine != (if removed { 0 } else { routine_update.routine })
            }
        };

    if mismatch {
        eprintln!(
            "MISMATCH(red_soldier_routine_01) frame={frame} pc={:04X} in=(frame={:02X} frame_counter={:02X} attrs={:02X} var_2={:02X} x={:02X} y={:02X} routine={:02X}): expected {:?}, got frame={real_frame:02X} sprites={real_sprites:02X} sprite_attr={real_sprite_attr:02X} x={real_x_pos:02X} y={real_y_pos:02X} var_1={real_var_1:02X} attack_delay={real_attack_delay:02X} routine={real_routine:02X}",
            cpu.pc, ctx.frame, ctx.frame_counter, ctx.attributes, ctx.var_2, ctx.x_pos, ctx.y_pos, ctx.routine, expected
        );
    }
}
