/// Captured inputs for one real `flying_capsule_routine_02` call
/// (`$8376`).
#[derive(Clone)]
pub struct FlyingCapsuleRoutine02Ctx {
    pub x: usize,
    pub current_level: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub attributes: u8,
    pub enemy_routine: [u8; contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT],
}

pub fn verify_flying_capsule_routine_02(
    ctx: FlyingCapsuleRoutine02Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::flying_capsule::flying_capsule_routine_02;

    let x = ctx.x;
    let expected = flying_capsule_routine_02(prg_rom, &ctx.enemy_routine, ctx.current_level, ctx.x_pos, ctx.y_pos, ctx.attributes);
    *checked += 1;

    let real_attributes = bus.ram[0x5A8 + x];
    let real_routine = bus.ram[0x4B8 + x];
    let real_type = bus.ram[0x528 + x];

    let mut mismatch = real_attributes != expected.attributes || real_routine != expected.routine || real_type != expected.enemy_type;

    if let Some(explosion) = &expected.explosion {
        let s = explosion.slot as usize;
        mismatch |= bus.ram[0x528 + s] != explosion.enemy_type
            || bus.ram[0x598 + s] != explosion.fields.state_width
            || bus.ram[0x33E + s] != explosion.fields.x_pos
            || bus.ram[0x324 + s] != explosion.fields.y_pos
            || bus.ram[0x4B8 + s] != explosion.routine;
    }

    if mismatch {
        eprintln!(
            "MISMATCH(flying_capsule_routine_02) frame={frame} pc={:04X} in=(level={:02X} x={:02X} y={:02X} attrs={:02X}): expected {:?}, got attrs={real_attributes:02X} routine={real_routine:02X} type={real_type:02X}",
            cpu.pc, ctx.current_level, ctx.x_pos, ctx.y_pos, ctx.attributes, expected
        );
    }
}
