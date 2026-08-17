/// Captured inputs for one real `four_soldiers_routine_01` call
/// (`$954c`).
#[derive(Clone)]
pub struct FourSoldiersRoutine01Ctx {
    pub x: usize,
    pub current_level: u8,
    pub attack_flag: u8,
    pub animation_delay: u8,
    pub soldier_index: u8,
    pub times_fired: u8,
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    pub x_pos: u8,
    pub y_pos: u8,
    pub routine: u8,
    pub enemy_routine: [u8; contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT],
}

pub fn verify_four_soldiers_routine_01(
    ctx: FourSoldiersRoutine01Ctx,
    prg_rom: &[u8],
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::four_soldiers::{four_soldiers_routine_01, FourSoldiersRoutine01Outcome};

    let x = ctx.x;
    let expected = four_soldiers_routine_01(
        prg_rom,
        &ctx.enemy_routine,
        ctx.current_level,
        ctx.attack_flag,
        ctx.animation_delay,
        ctx.soldier_index,
        ctx.times_fired,
        ctx.x_vel_fract,
        ctx.x_vel_fast,
        ctx.x_pos,
        ctx.y_pos,
        ctx.routine,
    );
    *checked += 1;

    let real_animation_delay = bus.ram[0x538 + x];
    let real_x_vel_fract = bus.ram[0x518 + x];
    let real_x_vel_fast = bus.ram[0x508 + x];
    let real_routine = bus.ram[0x4B8 + x];
    let real_sprites = bus.ram[0x30A + x];

    let mismatch = match &expected {
        FourSoldiersRoutine01Outcome::Waiting { animation_delay } => real_animation_delay != *animation_delay,
        FourSoldiersRoutine01Outcome::Fired { animation_delay, .. } => real_animation_delay != *animation_delay,
        FourSoldiersRoutine01Outcome::Advanced { x_velocity, delayed_routine } => {
            let vel_ok = match x_velocity {
                Some((fr, fa)) => real_x_vel_fract == *fr && real_x_vel_fast == *fa,
                None => true,
            };
            !vel_ok
                || real_animation_delay != delayed_routine.animation_delay
                || real_routine != delayed_routine.routine_update.routine
                || delayed_routine.routine_update.sprites.map(|s| real_sprites != s).unwrap_or(false)
        }
    };

    if mismatch {
        eprintln!(
            "MISMATCH(four_soldiers_routine_01) frame={frame} pc={:04X} in=(delay={:02X} soldier_index={:02X} times_fired={:02X} x_vel=({:02X},{:02X}) x={:02X} y={:02X} routine={:02X}): expected {:?}, got animation_delay={real_animation_delay:02X} x_vel=({real_x_vel_fract:02X},{real_x_vel_fast:02X}) routine={real_routine:02X} sprites={real_sprites:02X}",
            cpu.pc, ctx.animation_delay, ctx.soldier_index, ctx.times_fired, ctx.x_vel_fract, ctx.x_vel_fast, ctx.x_pos, ctx.y_pos, ctx.routine, expected
        );
    }
}
