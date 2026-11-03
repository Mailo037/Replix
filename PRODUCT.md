# Product

## Register

product

## Users

PC gamers on Windows who want to keep moments from their games without thinking about it. Replix runs in the background while they play; they open the window afterwards, between or after matches, to find, trim and share clips. Mostly at a desk, often in a dim room.

## Product Purpose

A clip recorder written in Rust. A replay buffer keeps the last N seconds (configurable) of a chosen source: the whole screen, a game, or one program. Alt+C saves it as a clip. Screenshots and continuous sessions (start, record until stopped) sit next to it. Every audio source (game, microphone, Discord, Spotify, system) is recorded on its own track, so each one can be muted per clip. Clips can be kept as drafts, trimmed and exported.

Success: saving a clip takes one key press, and finding and cleaning it up takes seconds.

## Brand Personality

Lively, playful, unobtrusive. Small animations and a bit of character, like Hamstr, while the tool itself disappears into the task.

References:
- Hamstr: calm dark surfaces, one accent, ASCII only as an accent.
- Raycast / Linear: minimal and fast, controls only where they are needed.
- Apple Photos / macOS: the library is a grid of pictures, editing opens on top of it.

## Anti-references

- Landing-page layouts: a big bold app name at the top left, hero numbers.
- Bars at the top or bottom that list hotkeys or what to press; settings spread over the main screen.
- Whole-screen ASCII or terminal looks, box-drawing borders.
- Busy gamer UIs: neon, RGB gradients, dense panels.

## Design Principles

1. The library is home: open the app, see your clips.
2. Settings stay out of the way: small, reachable, never spread over the main screen.
3. One obvious action per screen.
4. Alive, not loud: motion shows state (buffer running, saving), never decoration.
5. Understandable without a manual: plain words, standard controls.

## Accessibility & Inclusion

WCAG 2.2 AA contrast. Every effect respects reduced motion. State is never shown by colour alone. Everything works with the keyboard.
