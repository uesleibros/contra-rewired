/// Captured inputs for one real `red_soldier_routine_02` call (`$a2bb`).
#[derive(Clone)]
pub struct RedSoldierRoutine02Ctx {
    pub x: usize,
    pub current_level: u8,
    pub attack_flag: u8,
    pub attack_delay: u8,
    pub var_1: u8,
    pub var_2: u8,
    pub sprite_attr: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub player_state: [u8; 2],
    pub sprite_y_pos: [u8; 2],
    pub sprite_x_pos: [u8; 2],
    pub level_location_type: u8,
    pub routine: u8,
    pub enemy_routine: [u8; contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT],
}

pub fn verify_red_soldier_routine_02(
    ctx: RedSoldierRoutine02Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::red_blue_soldier::{red_soldier_routine_02, RedSoldierRoutine02Outcome};

    let x = ctx.x;
    let expected = red_soldier_routine_02(
        prg_rom, &ctx.enemy_routine, ctx.current_level, ctx.attack_flag, ctx.attack_delay, ctx.var_1, ctx.var_2,
        ctx.sprite_attr, ctx.x_pos, ctx.y_pos, ctx.player_state, ctx.sprite_y_pos, ctx.sprite_x_pos,
        ctx.level_location_type, ctx.routine,
    );
    *checked += 1;

    let real_attack_delay = bus.ram[0x558 + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_sprites = bus.ram[0x30A + x];
    let real_var_1 = bus.ram[0x5B8 + x];
    let real_var_2 = bus.ram[0x5C8 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = match expected {
        RedSoldierRoutine02Outcome::Waiting { attack_delay, sprite_attr } => {
            real_attack_delay != attack_delay || sprite_attr.map(|sa| real_sprite_attr != sa).unwrap_or(false)
        }
        RedSoldierRoutine02Outcome::Fired(f) => {
            real_sprites != f.sprites || real_var_1 != f.var_1 || real_attack_delay != f.attack_delay || real_sprite_attr != f.sprite_attr
        }
        RedSoldierRoutine02Outcome::AllFired { var_2, routine_update } => {
            real_var_2 != var_2 || real_routine != routine_update.routine
        }
    };

    if mismatch {
        eprintln!(
            "MISMATCH(red_soldier_routine_02) frame={frame} pc={:04X} in=(attack_delay={:02X} var_1={:02X} var_2={:02X} sprite_attr={:02X} x={:02X} y={:02X} routine={:02X}): expected {:?}, got attack_delay={real_attack_delay:02X} sprite_attr={real_sprite_attr:02X} sprites={real_sprites:02X} var_1={real_var_1:02X} var_2={real_var_2:02X} routine={real_routine:02X}",
            cpu.pc, ctx.attack_delay, ctx.var_1, ctx.var_2, ctx.sprite_attr, ctx.x_pos, ctx.y_pos, ctx.routine, expected
        );
    }
}
