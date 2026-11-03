# Replix

**Never miss the moment.** Replix quietly keeps the last seconds of your game. Press <kbd>Alt</kbd> + <kbd>C</kbd> and they're saved as a clip, with every sound on its own track.

![Replix library with clips from several games](docs/images/library.png)

**[Download for Windows](https://github.com/Mailo037/Replix/releases/latest)** · Windows 10 and 11, 64-bit · Free

## Keep what just happened

Replix starts on its own when a game is in front and keeps a rolling buffer, from 15 seconds up to 10 minutes. Nothing to start, nothing to stop. When something great happens, press the key; the game keeps running and a small note in the corner tells you it's saved.

Instead of games, Replix can also watch the whole screen or a single app. For longer stretches, start a session and it records until you press the key again.

![Recording menu in the title bar](docs/images/status.png)

## Every sound on its own track

The game, Discord, Spotify and your microphone are kept apart. Mute your friends or turn the music down later, and keep the rest. Any app that plays sound gets a track of its own.

Pick your microphone, check that it's picking you up, and let noise suppression filter out fans, hum and keyboard clatter.

## Edit in seconds

Trim the start and end, split the clip and take out the parts you don't need. Turn tracks off or down, then export in full quality, small enough for Discord, or as a GIF. <kbd>Ctrl</kbd> + <kbd>Z</kbd> takes any step back.

![Editor with a part cut out and four audio tracks](docs/images/editor.png)

Switch to 9:16 for TikTok, Shorts and Reels, and drag the frame to where the action is.

![Editor in 9:16 with the frame over the car](docs/images/vertical.png)

## Share it

Copy a clip and paste it straight into Discord or a chat, drag it out of the library, or get a link that works for 72 hours. Screenshots open in a viewer you can zoom into.

## Set it up once

A short setup walks you through the clip length, what to record, the tracks and your microphone.

![Setup step explaining audio tracks](docs/images/setup-tracks.png)

Every game can have its own clip length, frame rate, quality and microphone setting, or be left out entirely. A space limit moves the oldest clips to the recycle bin; favorites and drafts stay.

![Settings](docs/images/settings.png)

## Keys

| Key | What it does |
| --- | --- |
| <kbd>Alt</kbd> + <kbd>C</kbd> | Save the last moments as a clip |
| <kbd>Alt</kbd> + <kbd>S</kbd> | Screenshot |
| <kbd>Alt</kbd> + <kbd>X</kbd> | Start or stop a session |

All three can be changed in Settings. In the editor: <kbd>Space</kbd> plays, <kbd>S</kbd> splits, <kbd>Del</kbd> removes a part, <kbd>I</kbd> and <kbd>O</kbd> set the start and end.

## Your clips stay yours

Everything is recorded and kept on your PC, in `Videos\Replix` unless you choose another folder. Nothing is uploaded unless you click **Get a link**, which sends that one clip to [Litterbox](https://litterbox.catbox.moe) for 72 hours. Replix checks GitHub for new versions and updates itself.

## Good to know

- Recording uses your graphics card's video encoder (NVIDIA, AMD or Intel) and falls back to the processor when there is none.
- Replix waits in the tray and can start with Windows.

Replix uses [FFmpeg](https://ffmpeg.org), licensed under the GPL v3. Its source is available at [ffmpeg.org](https://ffmpeg.org/download.html).
