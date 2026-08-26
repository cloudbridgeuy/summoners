//! Command-line parsing at the process boundary.

use std::net::IpAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Play Summoners against another local player.
#[derive(Debug, Parser)]
#[command(name = "summoners", version, about)]
pub struct App {
    #[command(subcommand)]
    pub command: Command,
}

/// One operation selected by the user.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Host a match for one other player.
    Serve(ServeArgs),
    /// Join a hosted match as one of two players.
    Play(PlayArgs),
    /// Replay a recorded match transcript.
    Replay(ReplayArgs),
}

/// Raw command-line values for hosting a match.
#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Address to listen on.
    #[arg(long, default_value = "127.0.0.1")]
    pub bind: IpAddr,

    /// Port to listen on. Left out, the operating system assigns a free port.
    #[arg(long)]
    pub port: Option<u16>,

    /// The two decks that face each other in hosting order.
    #[arg(long, required = true, num_args = 2)]
    pub decks: Vec<String>,
}

/// Raw command-line values for joining a hosted match.
#[derive(Debug, Args)]
pub struct PlayArgs {
    /// Seat to take: 1 or 2.
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=2))]
    pub player: u8,

    /// Port the host listens on.
    #[arg(long)]
    pub port: Option<u16>,
}

/// Raw command-line values for replaying a transcript.
#[derive(Debug, Args)]
pub struct ReplayArgs {
    /// Recorded transcript to replay.
    pub transcript: PathBuf,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use clap::Parser;

    use super::{App, Command};

    #[test]
    fn parser_supplies_serve_defaults() {
        let app = App::try_parse_from(["summoners", "serve", "--decks", "a", "b"])
            .expect("serve command line is valid");
        let Command::Serve(args) = app.command else {
            panic!("expected serve command");
        };
        assert_eq!(args.bind, std::net::IpAddr::from([127, 0, 0, 1]));
        assert_eq!(args.port, None);
        assert_eq!(args.decks, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn parser_accepts_bind_and_port_overrides_for_serve() {
        let app = App::try_parse_from([
            "summoners",
            "serve",
            "--bind",
            "192.168.1.10",
            "--port",
            "9000",
            "--decks",
            "a",
            "b",
        ])
        .expect("serve command line is valid");
        let Command::Serve(args) = app.command else {
            panic!("expected serve command");
        };
        assert_eq!(args.bind.to_string(), "192.168.1.10");
        assert_eq!(args.port, Some(9000));
    }

    #[test]
    fn parser_rejects_wrong_deck_counts() {
        assert!(
            App::try_parse_from(["summoners", "serve", "--decks", "a"]).is_err(),
            "one deck must not parse"
        );
        assert!(
            App::try_parse_from(["summoners", "serve", "--decks", "a", "b", "c"]).is_err(),
            "three decks must not parse"
        );
        assert!(
            App::try_parse_from(["summoners", "serve"]).is_err(),
            "missing decks must not parse"
        );
    }

    #[test]
    fn parser_accepts_both_player_seats() {
        for seat in 1..=2 {
            let app = App::try_parse_from(["summoners", "play", "--player", &seat.to_string()])
                .expect("play command line is valid");
            let Command::Play(args) = app.command else {
                panic!("expected play command");
            };
            assert_eq!(args.player, seat);
            assert_eq!(args.port, None);
        }
    }

    #[test]
    fn parser_rejects_player_seats_outside_one_and_two() {
        assert!(App::try_parse_from(["summoners", "play"]).is_err());
        assert!(App::try_parse_from(["summoners", "play", "--player", "3"]).is_err());
        assert!(App::try_parse_from(["summoners", "play", "--player", "0"]).is_err());
        assert!(App::try_parse_from(["summoners", "play", "--player", "-1"]).is_err());
    }

    #[test]
    fn parser_accepts_play_port_override() {
        let app = App::try_parse_from(["summoners", "play", "--player", "2", "--port", "40000"])
            .expect("play command line is valid");
        let Command::Play(args) = app.command else {
            panic!("expected play command");
        };
        assert_eq!(args.player, 2);
        assert_eq!(args.port, Some(40000));
    }

    #[test]
    fn parser_accepts_transcript_path_positionally() {
        let app = App::try_parse_from(["summoners", "replay", "matches/game.ndjson"])
            .expect("replay command line is valid");
        let Command::Replay(args) = app.command else {
            panic!("expected replay command");
        };
        assert_eq!(args.transcript.display().to_string(), "matches/game.ndjson");
    }

    #[test]
    fn parser_rejects_unknown_subcommands_and_missing_paths() {
        assert!(App::try_parse_from(["summoners", "explore"]).is_err());
        assert!(App::try_parse_from(["summoners", "replay"]).is_err());
    }
}
