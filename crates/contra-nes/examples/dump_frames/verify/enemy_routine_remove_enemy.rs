/// Captured inputs for one real `enemy_routine_remove_enemy` call
/// (`$e806`) - a real, shared enemy-routine-table entry used by dozens
/// of enemy types, not just the soldier, so this hook has no per-type
/// fields at all.
#[derive(Clone, Copy)]
pub struct EnemyRoutineRemoveEnemyCtx {
    pub x: usize,
    pub scroll_type: u8,
    pub frame_scroll: u8,
    pub x_pos: u8,
    pub y_pos: u8,
}

pub fn verify_enemy_routine_remove_enemy(
    ctx: EnemyRoutineRemoveEnemyCtx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::update_enemy_pos::enemy_routine_remove_enemy;

    let x = ctx.x;
    let expected = enemy_routine_remove_enemy(ctx.scroll_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos);
    *checked += 1;

    let real_x_pos = bus.ram[0x33E + x];
    let real_y_pos = bus.ram[0x324 + x];
    let real_sprites = bus.ram[0x30A + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch =
        real_x_pos != expected.scroll.x_pos || real_y_pos != expected.scroll.y_pos || real_sprites != 0 || real_routine != 0;

    if mismatch {
        eprintln!(
            "MISMATCH(enemy_routine_remove_enemy) frame={frame} pc={:04X} in=(scroll_type={:02X} frame_scroll={:02X} x={:02X} y={:02X}): expected {:?}, got x={real_x_pos:02X} y={real_y_pos:02X} sprites={real_sprites:02X} routine={real_routine:02X}",
            cpu.pc, ctx.scroll_type, ctx.frame_scroll, ctx.x_pos, ctx.y_pos, expected
        );
    }
}

