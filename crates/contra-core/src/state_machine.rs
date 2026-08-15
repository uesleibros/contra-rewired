//! Top-level game state, mirroring the role of `GAME_ROUTINE_INDEX` in the
//! original code (see `ram.asm`: "$18 - which part of the game routine to
//! execute"). Kept as an explicit enum + transition function rather than a
//! generic FSM library so every legal transition is visible in one place.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameRoutine {
    Boot,
    TitleScreen,
    Demo,
    StageIntro,
    Playing,
    Paused,
    LevelTransition,
    PlayerDeath,
    GameOver,
    Ending,
}

#[derive(Debug, Clone, Copy)]
pub enum GameEvent {
    BootComplete,
    StartPressed,
    DemoTimeout,
    StageIntroFinished,
    PlayerDied,
    LastLifeLost,
    ContinueAccepted,
    StageCleared,
    NextStageReady,
    FinalStageCleared,
    PausePressed,
    ResumePressed,
    ReturnToTitle,
}

impl GameRoutine {
    /// Applies one event, returning the new state. Invalid combinations are
    /// a no-op (returns `self`) rather than a panic, since input events can
    /// arrive slightly out of order across a frame boundary.
    pub fn transition(self, event: GameEvent) -> GameRoutine {
        use GameEvent::*;
        use GameRoutine::*;
        match (self, event) {
            (Boot, BootComplete) => TitleScreen,
            (TitleScreen, StartPressed) => StageIntro,
            (TitleScreen, DemoTimeout) => Demo,
            (Demo, StartPressed) => StageIntro,
            (Demo, DemoTimeout) => TitleScreen,
            (StageIntro, StageIntroFinished) => Playing,
            (Playing, PausePressed) => Paused,
            (Paused, ResumePressed) => Playing,
            (Paused, ReturnToTitle) => TitleScreen,
            (Playing, PlayerDied) => PlayerDeath,
            (PlayerDeath, ContinueAccepted) => StageIntro,
            (PlayerDeath, LastLifeLost) => GameOver,
            (Playing, StageCleared) => LevelTransition,
            (LevelTransition, NextStageReady) => StageIntro,
            (LevelTransition, FinalStageCleared) => Ending,
            (GameOver, StartPressed) => TitleScreen,
            (Ending, StartPressed) => TitleScreen,
            (state, _) => state,
        }
    }

    pub fn accepts_gameplay_input(self) -> bool {
        matches!(self, GameRoutine::Playing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boots_into_title() {
        assert_eq!(GameRoutine::Boot.transition(GameEvent::BootComplete), GameRoutine::TitleScreen);
    }

    #[test]
    fn pause_resume_round_trips() {
        let s = GameRoutine::Playing.transition(GameEvent::PausePressed);
        assert_eq!(s, GameRoutine::Paused);
        assert_eq!(s.transition(GameEvent::ResumePressed), GameRoutine::Playing);
    }

    #[test]
    fn unrelated_event_is_a_no_op() {
        assert_eq!(GameRoutine::TitleScreen.transition(GameEvent::PlayerDied), GameRoutine::TitleScreen);
    }

    #[test]
    fn death_spiral_reaches_game_over() {
        let mut s = GameRoutine::Playing;
        s = s.transition(GameEvent::PlayerDied);
        s = s.transition(GameEvent::LastLifeLost);
        assert_eq!(s, GameRoutine::GameOver);
    }
}
