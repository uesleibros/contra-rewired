/// Captured inputs for one real `grenade_routine_02` call (`$907c`).
#[derive(Clone, Copy)]
pub struct GrenadeRoutine02Ctx {
    pub x: usize,
    pub state_width: u8,
    pub sprite_attr: u8,
    pub sprites: u8,
    pub level_scrolling_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub routine: u8,
}

pub fn verify_grenade_routine_02(
    ctx: GrenadeRoutine02Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::grenade::grenade_routine_02;
    use contra_native::enemy::mortar_shot::MortarShotRoutine03Outcome;

    let x = ctx.x;
    let expected = grenade_routine_02(ctx.state_width, ctx.sprite_attr, ctx.sprites, ctx.level_scrolling_type, ctx.frame_scroll, ctx.x_pos, ctx.routine);
    *checked += 1;

    let real_score_collision = bus.ram[0x588 + x];
    let real_state_width = bus.ram[0x598 + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let base_ok = real_score_collision == expected.mortar_result.score_collision
        && real_state_width == expected.mortar_result.state_width
        && real_sprite_attr == expected.mortar_result.sprite_attr
        && real_y_pos == expected.y_pos
        && real_routine == expected.final_routine_update.routine;

    let outcome_ok = match &expected.mortar_result.outcome {
        MortarShotRoutine03Outcome::Removed(_) => true,
        MortarShotRoutine03Outcome::Hidden(h) => {
            let real_frame = bus.ram[0x568 + x];
            let real_sprites = bus.ram[0x30A + x];
            real_frame == h.enemy_frame && real_sprites == h.enemy_sprites
        }
    };

    let mismatch = !base_ok || !outcome_ok;

    if mismatch {
        eprintln!(
            "MISMATCH(grenade_routine_02) frame={frame} pc={:04X} in=(state_width={:02X} sprites={:02X} x={:02X} routine={:02X}): expected {:?}, got score_collision={real_score_collision:02X} state_width={real_state_width:02X} sprite_attr={real_sprite_attr:02X} y={real_y_pos:02X} routine={real_routine:02X}",
            cpu.pc, ctx.state_width, ctx.sprites, ctx.x_pos, ctx.routine, expected
        );
    }
}
