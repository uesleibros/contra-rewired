/// Captured inputs for one real `enemy_bullet_routine_00` call
/// (`$814f`).
#[derive(Clone, Copy)]
pub struct EnemyBulletRoutine00Ctx {
    pub x: usize,
    pub bullet_type: u8,
    pub routine: u8,
}

pub fn verify_enemy_bullet_routine_00(
    ctx: EnemyBulletRoutine00Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::enemy_bullet::enemy_bullet_routine_00;

    let x = ctx.x;
    let expected = enemy_bullet_routine_00(ctx.bullet_type, ctx.routine);
    *checked += 1;

    let real_score_collision = bus.ram[0x588 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = real_score_collision != expected.score_collision || real_routine != expected.routine_update.routine;

    if mismatch {
        eprintln!(
            "MISMATCH(enemy_bullet_routine_00) frame={frame} pc={:04X} in=(bullet_type={:02X} routine={:02X}): expected {:?}, got score_collision={real_score_collision:02X} routine={real_routine:02X}",
            cpu.pc, ctx.bullet_type, ctx.routine, expected
        );
    }
}
