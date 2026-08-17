/// Captured inputs for one real `jumping_soldier_routine_04` call
/// (`$9437`).
#[derive(Clone)]
pub struct JumpingSoldierRoutine04Ctx {
    pub x: usize,
    pub current_level: u8,
    pub attributes: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub routine: u8,
    pub enemy_routine: [u8; contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT],
}

pub fn verify_jumping_soldier_routine_04(
    ctx: JumpingSoldierRoutine04Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::jumping_soldier::{jumping_soldier_routine_04, JumpingSoldierRoutine04Outcome};

    let x = ctx.x;
    let expected = jumping_soldier_routine_04(prg_rom, &ctx.enemy_routine, ctx.current_level, ctx.attributes, ctx.x_pos, ctx.y_pos, ctx.routine);
    *checked += 1;

    let real_attributes = bus.ram[0x5A8 + x];
    let real_routine = bus.ram[0x4B8 + x];
    let real_sprites = bus.ram[0x30A + x];

    let mismatch = match &expected {
        JumpingSoldierRoutine04Outcome::AdvancedOnly(routine_update) => {
            real_routine != routine_update.routine || routine_update.sprites.map(|s| real_sprites != s).unwrap_or(false)
        }
        JumpingSoldierRoutine04Outcome::ExplodedAndDroppedWeapon { attributes_after_shift, play_result } => {
            let mut m = real_attributes != play_result.attributes || real_routine != play_result.routine;
            if let Some(explosion) = &play_result.explosion {
                let s = explosion.slot as usize;
                m |= bus.ram[0x528 + s] != explosion.enemy_type
                    || bus.ram[0x598 + s] != explosion.fields.state_width
                    || bus.ram[0x33E + s] != explosion.fields.x_pos
                    || bus.ram[0x324 + s] != explosion.fields.y_pos
                    || bus.ram[0x4B8 + s] != explosion.routine;
            }
            let _ = attributes_after_shift;
            m
        }
    };

    if mismatch {
        eprintln!(
            "MISMATCH(jumping_soldier_routine_04) frame={frame} pc={:04X} in=(attrs={:02X} x={:02X} y={:02X} routine={:02X}): expected {:?}, got attrs={real_attributes:02X} routine={real_routine:02X} sprites={real_sprites:02X}",
            cpu.pc, ctx.attributes, ctx.x_pos, ctx.y_pos, ctx.routine, expected
        );
    }
}
