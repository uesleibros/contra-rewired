/// Captured inputs for one real `indoor_soldier_gen_routine_01` call
/// (`$8d28`).
#[derive(Clone)]
pub struct IndoorSoldierGenRoutine01Ctx {
    pub x: usize,
    pub current_level: u8,
    pub frame_counter: u8,
    pub grenade_launcher_flag: u8,
    pub animation_delay: u8,
    pub attributes: u8,
    pub level_screen_number: u8,
    pub var_1: u8,
    pub indoor_enemy_attack_count: u8,
    pub enemy_routine: [u8; contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT],
}

pub fn verify_indoor_soldier_gen_routine_01(
    ctx: IndoorSoldierGenRoutine01Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::indoor_soldier_gen::{indoor_soldier_gen_routine_01, IndoorSoldierGenRoutine01Outcome};

    let x = ctx.x;
    let expected = indoor_soldier_gen_routine_01(
        prg_rom,
        &ctx.enemy_routine,
        ctx.current_level,
        ctx.frame_counter,
        ctx.grenade_launcher_flag,
        ctx.animation_delay,
        ctx.attributes,
        ctx.level_screen_number,
        ctx.var_1,
        ctx.indoor_enemy_attack_count,
    );
    *checked += 1;

    let real_animation_delay = bus.ram[0x538 + x];
    let real_var_1 = bus.ram[0x5B8 + x];
    let real_attack_count = bus.ram[0x88];
    let real_routine = bus.ram[0x4B8 + x];
    let real_sprites = bus.ram[0x30A + x];

    let mismatch = match &expected {
        IndoorSoldierGenRoutine01Outcome::EvenFrame | IndoorSoldierGenRoutine01Outcome::GrenadeLauncherOnScreen => false,
        IndoorSoldierGenRoutine01Outcome::StillWaiting { animation_delay } => real_animation_delay != *animation_delay,
        IndoorSoldierGenRoutine01Outcome::RoundsExhausted { indoor_enemy_attack_count, .. } => {
            real_attack_count != *indoor_enemy_attack_count || real_routine != 0 || real_sprites != 0
        }
        IndoorSoldierGenRoutine01Outcome::Entry { indoor_enemy_attack_count, result } => {
            let count_ok = indoor_enemy_attack_count.map(|c| real_attack_count == c).unwrap_or(true);
            let mut m = !count_ok || real_animation_delay != result.animation_delay || real_var_1 != result.var_1;
            for spawn in &result.spawns {
                let s = spawn.slot as usize;
                m |= bus.ram[0x528 + s] != spawn.enemy_type
                    || bus.ram[0x5A8 + s] != spawn.fields.attributes
                    || bus.ram[0x5B8 + s] != spawn.fields.var_1
                    || bus.ram[0x578 + s] != spawn.hp
                    || bus.ram[0x4B8 + s] == 0; // real ENEMY_ROUTINE must be nonzero (initialize_enemy sets it to 1)
            }
            m
        }
    };

    if mismatch {
        eprintln!(
            "MISMATCH(indoor_soldier_gen_routine_01) frame={frame} pc={:04X} in=(frame_counter={:02X} glf={:02X} delay={:02X} attrs={:02X} screen={:02X} var_1={:02X} attack_count={:02X}): expected {:?}, got animation_delay={real_animation_delay:02X} var_1={real_var_1:02X} attack_count={real_attack_count:02X} routine={real_routine:02X} sprites={real_sprites:02X}",
            cpu.pc,
            ctx.frame_counter,
            ctx.grenade_launcher_flag,
            ctx.animation_delay,
            ctx.attributes,
            ctx.level_screen_number,
            ctx.var_1,
            ctx.indoor_enemy_attack_count,
            expected
        );
    }
}
