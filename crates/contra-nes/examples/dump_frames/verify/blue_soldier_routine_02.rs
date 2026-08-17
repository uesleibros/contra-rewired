/// Captured inputs for one real `blue_soldier_routine_02` call (`$a1f7`).
#[derive(Clone, Copy)]
pub struct BlueSoldierRoutine02Ctx {
    pub x: usize,
    pub animation_delay: u8,
    pub frame: u8,
    pub attributes: u8,
    pub sprite_attr: u8,
    pub state_width: u8,
    pub routine: u8,
}

pub fn verify_blue_soldier_routine_02(
    ctx: BlueSoldierRoutine02Ctx,
    cpu: &contra_nes::cpu::Cpu,
    bus: &contra_nes::bus::NesBus,
    frame: u32,
    checked: &mut u64,
) {
    use contra_native::enemy::red_blue_soldier::{blue_soldier_routine_02, BlueSoldierRoutine02Outcome};

    let x = ctx.x;
    let expected = blue_soldier_routine_02(ctx.animation_delay, ctx.frame, ctx.attributes, ctx.sprite_attr, ctx.state_width, ctx.routine);
    *checked += 1;

    let real_delay = bus.ram[0x538 + x];
    let real_frame = bus.ram[0x568 + x];
    let real_sprites = bus.ram[0x30A + x];
    let real_sprite_attr = bus.ram[0x358 + x];
    let real_state_width = bus.ram[0x598 + x];
    let real_x_fract = bus.ram[0x518 + x];
    let real_x_fast = bus.ram[0x508 + x];
    let real_y_fract = bus.ram[0x4F8 + x];
    let real_y_fast = bus.ram[0x4E8 + x];
    let real_routine = bus.ram[0x4B8 + x];

    let mismatch = match expected {
        BlueSoldierRoutine02Outcome::Waiting { animation_delay } => real_delay != animation_delay,
        BlueSoldierRoutine02Outcome::Animating { enemy_frame, sprites, animation_delay } => {
            real_frame != enemy_frame || real_sprites != sprites || real_delay != animation_delay
        }
        BlueSoldierRoutine02Outcome::JumpStart { sprites, sprite_attr, state_width, x_velocity, y_velocity, delayed_routine } => {
            real_sprites != sprites
                || real_sprite_attr != sprite_attr
                || real_state_width != state_width
                || real_x_fract != x_velocity.0
                || real_x_fast != x_velocity.1
                || real_y_fract != y_velocity.0
                || real_y_fast != y_velocity.1
                || real_delay != delayed_routine.animation_delay
                || real_routine != delayed_routine.routine_update.routine
        }
    };

    if mismatch {
        eprintln!(
            "MISMATCH(blue_soldier_routine_02) frame={frame} pc={:04X} in=(delay={:02X} frame={:02X} attrs={:02X} sprite_attr={:02X} state_width={:02X} routine={:02X}): expected {:?}, got delay={real_delay:02X} frame={real_frame:02X} sprites={real_sprites:02X} sprite_attr={real_sprite_attr:02X} state_width={real_state_width:02X} xvel=({real_x_fract:02X},{real_x_fast:02X}) yvel=({real_y_fract:02X},{real_y_fast:02X}) routine={real_routine:02X}",
            cpu.pc, ctx.animation_delay, ctx.frame, ctx.attributes, ctx.sprite_attr, ctx.state_width, ctx.routine, expected
        );
    }
}
