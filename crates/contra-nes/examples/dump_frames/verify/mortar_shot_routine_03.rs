/// Captured inputs for one real `mortar_shot_routine_03` call (`$e752`,
/// fixed bank - also reused by ice grenades).
#[derive(Clone, Copy)]
pub struct MortarShotRoutine03Ctx {
    pub x: usize,
    pub state_width: u8,
    pub sprite_attr: u8,
    pub sprites: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub routine: u8,
}

pub fn verify_mortar_shot_routine_03(
    ctx: MortarShotRoutine03Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::mortar_shot::{mortar_shot_routine_03, MortarShotRoutine03Outcome};

    let x = ctx.x;
    let expected = mortar_shot_routine_03(ctx.state_width, ctx.sprite_attr, ctx.sprites, ctx.level_scrolling_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos, ctx.routine);
    *checked += 1;

    let real_score_collision = bus.ram[0x588 + x];
    let real_state_width = bus.ram[0x598 + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let base_ok = real_score_collision == expected.score_collision && real_state_width == expected.state_width && real_sprite_attr == expected.sprite_attr;

    let mismatch = !base_ok
        || match &expected.outcome {
            MortarShotRoutine03Outcome::Removed(_) => false,
            MortarShotRoutine03Outcome::Hidden(h) => {
                let real_frame = bus.ram[0x568 + x];
                let real_sprites = bus.ram[0x30A + x];
                let real_x_pos = bus.ram[0x33E + x];
                let real_y_pos = bus.ram[0x324 + x];
                let real_animation_delay = bus.ram[0x538 + x];
                real_frame != h.enemy_frame
                    || real_sprites != h.enemy_sprites
                    || real_x_pos != h.scroll.x_pos
                    || real_y_pos != h.scroll.y_pos
                    || real_animation_delay != h.delayed_routine.animation_delay
                    || real_routine != h.delayed_routine.routine_update.routine
            }
        };

    if mismatch {
        eprintln!(
            "MISMATCH(mortar_shot_routine_03) frame={frame} pc={:04X} in=(state_width={:02X} sprite_attr={:02X} sprites={:02X} x={:02X} y={:02X} routine={:02X}): expected {:?}, got score_collision={real_score_collision:02X} state_width={real_state_width:02X} sprite_attr={real_sprite_attr:02X} routine={real_routine:02X}",
            cpu.pc, ctx.state_width, ctx.sprite_attr, ctx.sprites, ctx.x_pos, ctx.y_pos, ctx.routine, expected.outcome
        );
    }
}
