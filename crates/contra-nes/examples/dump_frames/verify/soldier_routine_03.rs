/// Captured inputs for one real `soldier_routine_03` call (`$8803`). See
/// `VERIFY_SOLDIER_ROUTINE_03`'s comment in `main` for the real exits and
/// the nested-return disambiguation the `AllFired` path needs.
#[derive(Clone)]
pub struct SoldierRoutine03Ctx {
    pub x: usize,
    pub current_level: u8,
    pub attack_flag: u8,
    pub attributes: u8,
    pub attack_delay: u8,
    pub var_3: u8,
    pub var_2: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub var_1: u8,
    pub scroll_type: u8,
    pub frame_scroll: u8,
    pub routine: u8,
    pub enemy_routine: [u8; contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT],
}

pub fn verify_soldier_routine_03(
    ctx: SoldierRoutine03Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::soldier::{soldier_routine_03, SoldierRoutine03Outcome};

    let x = ctx.x;
    let expected = soldier_routine_03(
        prg_rom, &ctx.enemy_routine, ctx.current_level, ctx.attack_flag, ctx.attributes, ctx.attack_delay, ctx.var_3,
        ctx.var_2, ctx.x_pos, ctx.y_pos, ctx.var_1, ctx.scroll_type, ctx.frame_scroll, ctx.routine,
    );
    *checked += 1;

    let real_score_collision = bus.ram[0x588 + x];
    let real_frame = bus.ram[0x568 + x];
    let real_attack_delay = bus.ram[0x558 + x];
    let real_var_3 = bus.ram[0x5D8 + x];
    let real_var_1 = bus.ram[0x5B8 + x];
    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_sprites = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = match expected {
        SoldierRoutine03Outcome::Waiting(w) => {
            let score_ok = w.score_collision.map(|v| real_score_collision == v).unwrap_or(true);
            let removed = w.tail.scroll.should_remove;
            let expected_sprites = if removed { 0 } else { w.tail.sprite.sprite };
            let expected_routine = if removed { 0 } else { ctx.routine };
            !score_ok
                || real_frame != w.enemy_frame
                || real_attack_delay != w.attack_delay
                || real_x_pos != w.tail.scroll.x_pos
                || real_y_pos != w.tail.scroll.y_pos
                || real_sprites != expected_sprites
                || real_sprite_attr != w.tail.sprite.sprite_attr
                || real_var_1 != w.tail.sprite.var_1
                || real_routine != expected_routine
        }
        SoldierRoutine03Outcome::Fired(f) => {
            let score_ok = f.score_collision.map(|v| real_score_collision == v).unwrap_or(true);
            let removed = f.tail.scroll.should_remove;
            let expected_sprites = if removed { 0 } else { f.tail.sprite.sprite };
            let expected_routine = if removed { 0 } else { ctx.routine };
            !score_ok
                || real_frame != f.enemy_frame
                || real_attack_delay != f.attack_delay
                || real_var_3 != f.var_3
                || real_x_pos != f.tail.scroll.x_pos
                || real_y_pos != f.tail.scroll.y_pos
                || real_sprites != expected_sprites
                || real_sprite_attr != f.tail.sprite.sprite_attr
                || real_var_1 != f.tail.sprite.var_1
                || real_routine != expected_routine
        }
        SoldierRoutine03Outcome::AllFired(a) => {
            let removed = a.tail.scroll.should_remove;
            let expected_sprites = if removed { 0 } else { a.tail.sprite.sprite };
            let expected_routine = if removed { 0 } else { a.routine_update.routine };
            real_score_collision != 0x10
                || real_frame != 0x00
                || real_var_3 != 0x00
                || real_x_pos != a.tail.scroll.x_pos
                || real_y_pos != a.tail.scroll.y_pos
                || real_sprites != expected_sprites
                || real_sprite_attr != a.tail.sprite.sprite_attr
                || real_var_1 != a.tail.sprite.var_1
                || real_routine != expected_routine
        }
    };

    if mismatch {
        eprintln!(
            "MISMATCH(soldier_routine_03) frame={frame} pc={:04X} in=(attack_flag={:02X} attrs={:02X} attack_delay={:02X} var_3={:02X} var_2={:02X} x={:02X} y={:02X} var_1={:02X} scroll_type={:02X} frame_scroll={:02X} routine={:02X}): expected {:?}, got score_collision={real_score_collision:02X} frame={real_frame:02X} attack_delay={real_attack_delay:02X} var_3={real_var_3:02X} var_1={real_var_1:02X} x={real_x_pos:02X} y={real_y_pos:02X} sprites={real_sprites:02X} sprite_attr={real_sprite_attr:02X} routine={real_routine:02X}",
            cpu.pc, ctx.attack_flag, ctx.attributes, ctx.attack_delay, ctx.var_3, ctx.var_2, ctx.x_pos, ctx.y_pos, ctx.var_1, ctx.scroll_type, ctx.frame_scroll, ctx.routine, expected
        );
    }
}

