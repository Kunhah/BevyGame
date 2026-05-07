# BevyGame
## Local data editors

Ability and dialogue editing has a no-build local web UI backed by a small
Rust HTTP server (`src/bin/editor_server.rs`). All on-disk data files use the
[RON](https://github.com/ron-rs/ron) format; the server is the single source
of truth for parsing/serializing RON, and the JS frontend talks to it over a
small JSON API.

Authored game data now lives under `assets/data/` instead of `src/`, split by
content type:

- `assets/data/abilities/`
- `assets/data/characters/`
- `assets/data/dialogues/`
- `assets/data/skills/`
- `assets/data/*.ron` for shared registries such as quests, economy, and AI

Run:

```bash
./scripts/serve_editors.sh
```

Then open:

```text
http://127.0.0.1:8000/
```

The first run compiles the editor server in release mode (set `PROFILE=debug`
to skip optimisation while iterating).

Notes:

- `Reload from disk` fetches the latest RON file from the server.
- `Save (RON)` writes the JS state back to the canonical RON file on disk
  via the server (POST /api/abilities or /api/dialogues).
- `Download RON` produces a local JSON snapshot of the in-memory data; use
  it as a backup or to diff state offline.
- `Validate` runs the same id/cycle checks that the previous JSON editor had.

## Performance settings

The Pause menu (Esc) and Main Menu now expose a **Settings** panel with
toggles for the most expensive optional features:

- **Lighting raymarch** — turn off the shader's shadow raymarch loop.
- **Light visibility CPU pass** — disable the per-frame
  `apply_light_visibility` system (O(entities × occluders) line-of-sight
  test).
- **Visual occluder fade** — disable the per-frame fade-when-covered system.
- **Occluder motion log** — disable the once-per-second debug log.

Tweak these if you are CPU- or shader-bound; defaults match the previous
behaviour. Toggles are persisted to `saves/settings.ron` and reloaded on
the next launch.

## Quest framework

`src/quests.rs` is a data-driven quest system. Quests live in
[`assets/data/quests.ron`](assets/data/quests.ron) and are loaded into a
`QuestRegistry` at startup.

A quest definition has:

- `objectives: [(id, description, kind, required)]` — each `kind` is one of
  `Kill { enemy_id }`, `Reach { area_id }`, `Talk { dialogue_id }`,
  `DialogueChoice { event_id }`, `ReputationAtLeast { target, threshold }`,
  or `ManualFlag { tag }`. `required` is the progress count needed to
  complete (1 for binary kinds, N for kill counters).
- `preconditions` — `QuestCompleted(id)`, `ReputationAtLeast`,
  `PlayerLevelAtLeast(n)`, `FlagSet("name")`. A quest with `auto_offer: true`
  activates automatically as soon as its preconditions are met.
- `rewards` — `Experience`, `Reputation`, `Flag`, `UnlockQuest(id)`. Awarded
  on completion via the `QuestRewardGrantedEvent` stream.

Game events flow into objective progress automatically: `DeathEvent`,
`AreaChanged`, dialogue completion, dialogue choices, and
`ReputationChangeEvent`. Hand-fired triggers go through `ManualFlagEvent`
or the legacy `OnItemPickup` / `OnDeath` / `OnReach` hook components.

Quest state is in the `QuestLog` resource (`quests`, `offered`, `completed`,
`failed`); boolean flags are in `QuestFlags`. Add a new quest at runtime by
sending `AddQuestEvent { quest_id }`.
