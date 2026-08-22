use std::io::Write;

use summoners_core::domain::state::GameState;

use crate::{
    codec::encode_record,
    error::RecordingError,
    state::{StateDigestV1, StateProjectionV1},
    wire::{HeaderMetadataV1, HeaderV1, MatchCreatedV1, RecordV1, SetRequirementV1},
};

/// An active match recording at a durable initial checkpoint.
pub struct RecordedMatch<W> {
    writer: W,
    state: GameState,
}

impl<W: Write> RecordedMatch<W> {
    /// Write and flush the initial transcript records before an active handle
    /// becomes available.
    pub fn start(
        mut writer: W,
        metadata: HeaderMetadataV1,
        required_sets: Vec<SetRequirementV1>,
        initial_state: GameState,
    ) -> Result<Self, RecordingError> {
        let records = prepare_start(metadata, required_sets, &initial_state)?;
        let header = encode_record(&records.0)?;
        let created = encode_record(&records.1)?;

        write_record(&mut writer, &header)?;
        write_record(&mut writer, &created)?;
        flush_checkpoint(&mut writer)?;

        Ok(Self {
            writer,
            state: initial_state,
        })
    }

    /// Read the unchanged authoritative state owned by this recording.
    #[must_use]
    pub const fn state(&self) -> &GameState {
        &self.state
    }

    /// Return the caller's writer after recording work is complete.
    #[must_use]
    pub fn into_writer(self) -> W {
        self.writer
    }
}

fn prepare_start(
    metadata: HeaderMetadataV1,
    required_sets: Vec<SetRequirementV1>,
    state: &GameState,
) -> Result<(RecordV1, RecordV1), RecordingError> {
    let projection = StateProjectionV1::from_state(state);
    let digest = StateDigestV1::compute(&projection)?;
    Ok((
        RecordV1::Header(HeaderV1::new(metadata)),
        RecordV1::MatchCreated(Box::new(MatchCreatedV1::new(
            required_sets,
            projection,
            digest,
        ))),
    ))
}

fn write_record(writer: &mut impl Write, bytes: &[u8]) -> Result<(), RecordingError> {
    writer.write_all(bytes).map_err(RecordingError::Write)
}

fn flush_checkpoint(writer: &mut impl Write) -> Result<(), RecordingError> {
    writer.flush().map_err(RecordingError::Flush)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::{collections::VecDeque, io, sync::Arc};

    use summoners_core::domain::{
        cards::CardSet,
        ids::PlayerId,
        state::{GameState, GameStatus, ManaBank, PerPlayer, Phase, PlayerState, TurnState},
    };

    use super::*;

    fn state() -> GameState {
        let player = PlayerState {
            main: None,
            bench: [None, None, None],
            deck: vec![],
            hand: vec![],
            prizes: vec![],
            discard: vec![],
            mana: ManaBank::default(),
            main_losses: 0,
            enchantments: vec![],
        };
        GameState {
            players: PerPlayer::new(player.clone(), player),
            coin: None,
            turn: TurnState {
                active_player: PlayerId::One,
                phase: Phase::Main,
                window: None,
                normal_attack_used: false,
                normal_retreat_used: false,
                spell_played_this_turn: PerPlayer::new(false, false),
            },
            stack: vec![],
            stack_segment_bases: vec![],
            work: VecDeque::new(),
            pending: None,
            status: GameStatus::Playing,
            cards: Arc::new(CardSet::new(vec![])),
        }
    }

    #[test]
    fn prepare_start_builds_header_and_match_created_records() {
        let records = prepare_start(
            HeaderMetadataV1::new(),
            vec![SetRequirementV1 {
                set: "foundations".to_string(),
                revision: 1,
            }],
            &state(),
        )
        .expect("the state is serializable");

        assert!(matches!(records.0, RecordV1::Header(_)));
        assert!(matches!(records.1, RecordV1::MatchCreated(_)));
    }

    #[test]
    fn write_record_maps_a_sink_failure_to_the_write_variant() {
        let mut writer = AlwaysFails;
        let error = write_record(&mut writer, b"record").expect_err("the write fails");
        assert!(matches!(error, RecordingError::Write(_)));
    }

    #[test]
    fn flush_checkpoint_maps_a_sink_failure_to_the_flush_variant() {
        let mut writer = FlushFails;
        let error = flush_checkpoint(&mut writer).expect_err("the flush fails");
        assert!(matches!(error, RecordingError::Flush(_)));
    }

    struct AlwaysFails;

    impl Write for AlwaysFails {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("injected write failure"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct FlushFails;

    impl Write for FlushFails {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("injected flush failure"))
        }
    }
}
