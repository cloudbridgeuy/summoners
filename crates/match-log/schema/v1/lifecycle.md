# Summoners match log lifecycle, version 1

The JSON Schema files define each record. This document defines the rules that apply across records in one strict NDJSON transcript.

## JSON integer tokens

Every integer-valued wire field uses a JSON integer token. Floating-point spellings such as `1.0` are invalid, even when their mathematical value is an integer.

## Record order

The record order is `header`, `match_created`, then one or more action steps, `final_state`, and `match_completed`. A step contains `action`, zero or more `event` records for an accepted result, and exactly one `step_completed` or `step_rejected` result.

## Global sequence

`sequence` starts at 0 on `header` and increases by exactly 1 for every line. `match_created` has sequence 1. Missing, repeated, or reordered sequence values are invalid.

## Action steps

`step` starts at 1 on the first `action` and increases by exactly 1 after each step result. Each event and step result has the same step value as its action. A second action cannot start before the prior step result.

## Event indexes and counts

For each accepted step, event `index` starts at 0 and increases by exactly 1 in wire order. `step_completed.event_count` equals the number of event records in that step. An accepted step can have an event count of 0.

## Rejected steps

A rejected step has no event records. Its `state_digest` equals the digest before the rejected action, which proves that the authoritative state did not change.

## Terminal records

### Exactly one `game_ended` event

The transcript contains exactly one `game_ended` event. It is in the final accepted step. No action or other step result can follow that terminal step.

### Terminal outcome agreement

The winner and loss reason in the `game_ended` event, the ended status in `final_state`, and `match_completed` are equal. `final_state` must contain an ended state; a playing or broken state is invalid.

### Digest agreement

`match_created.state_digest` is the digest of `initial_state`. Each accepted step establishes its `step_completed.state_digest`. Each rejected step keeps the prior digest. The terminal step digest, the computed digest of `final_state`, `final_state.state_digest`, and `match_completed.state_digest` are equal.

### Total counts

`match_completed.step_count` equals the total number of accepted and rejected steps. `match_completed.event_count` equals the total number of event records in all accepted steps.

No record can follow `match_completed`. A transcript that stops before `match_completed`, has more than one terminal event, or omits any required terminal record is incomplete.
