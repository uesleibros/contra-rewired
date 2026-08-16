/*
 * extras.c - minimal ContraRecomp runner hooks (experimental viability
 * test only, not a real game port). Every function is the documented
 * no-op default from game_extras.h's own "empty implementations" note.
 */
#include "game_extras.h"
#include "nes_runtime.h"
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>

void func_RESET(void);
void func_NMI(void);

/* Globals main_runner.c/debug_server.c extern - every game's extras.c
 * must define these (see FaxanaduRecomp's extras.c for the pattern). */
const char *g_rom_path_for_extras = NULL;
int         g_watchdog_triggered = 0;
uint32_t    g_watchdog_frame     = 0;
const char *g_watchdog_stack_dump = "";

const char *game_get_name(void) { return "Contra (experimental)"; }
void game_run_nmi(void) { func_NMI(); }
void game_run_main(void) { func_RESET(); }
void game_on_init(void) {}
void game_on_frame(uint64_t frame_count) { (void)frame_count; }
void game_post_nmi(uint64_t frame_count) {
    static uint8_t last_routine = 0xFF;
    uint8_t routine  = g_ram[0x18]; /* GAME_ROUTINE_INDEX */
    uint8_t initflag = g_ram[0x19]; /* GAME_ROUTINE_INIT_FLAG */
    uint8_t delay    = g_ram[0x2A]; /* DELAY_TIME_LOW_BYTE */
    uint8_t hscroll  = g_ram[0xFD]; /* HORIZONTAL_SCROLL */
    uint8_t diff     = g_ram[0xF5]; /* CONTROLLER_STATE_DIFF (player 1) */
    if (routine != last_routine || diff != 0 || frame_count < 20 || routine == 1) {
        printf("[Diag] frame=%llu ROUTINE=%02x INIT_FLAG=%02x DELAY=%02x HSCROLL=%02x DIFF=%02x\n",
               (unsigned long long)frame_count, routine, initflag, delay, hscroll, diff);
        last_routine = routine;
    }
}
int game_handle_arg(const char *key, const char *val) { (void)key; (void)val; return 0; }
const char *game_arg_usage(void) { return NULL; }
uint32_t game_get_expected_crc32(void) { return 0; }
int game_dispatch_override(uint16_t addr) { (void)addr; return 0; }
uint8_t game_ram_read_hook(uint16_t pc, uint16_t addr, uint8_t val) { (void)pc; (void)addr; return val; }
void game_post_render(uint32_t *framebuf) { (void)framebuf; }
void game_fill_frame_record(void *record) { (void)record; }
int game_handle_debug_cmd(const char *cmd, int id, const char *json) { (void)cmd; (void)id; (void)json; return 0; }
