# Keyboard shortcuts

This page lists MeetCue's global shortcuts and shows how to change them. The shortcuts work while your meeting app has focus.

## Default shortcuts

| Shortcut | Action |
| --- | --- |
| `Ctrl+Alt+Space` | Start or pause listening |
| `Ctrl+Alt+H` | Panel: full → mini bar → hidden |
| `Ctrl+Alt+[` | Previous question |
| `Ctrl+Alt+]` | Next question (in practice: skip the question) |
| `Ctrl+Alt+Enter` | Mark the current question answered |
| `Ctrl+Alt+Shift` + arrow keys | Move the panel up, down, left or right |
| `Ctrl+Alt+Shift+1`, `2`, `3` | Move the panel to saved spot 1, 2 or 3 (on an empty spot: save where it is now) |
| `Ctrl+Alt+Shift+P` | Hide the panel for 6 seconds; press again to bring it back sooner |
| `Ctrl+Alt+Shift+T` | Let clicks pass through the panel, or stop them passing through |

## Move the panel {#move-the-panel}

Put the panel right under your camera, so that reading a hint looks like looking at the other participants. Tap `Ctrl+Alt+Shift` with an arrow key to move the panel a little; hold the keys to keep it moving. Hold two arrows to move diagonally. The panel stays on screen, moves to a neighbouring screen when you push it past the side, and remembers where you left it.

## Saved spots {#saved-spots}

Most meetings have two or three good places for the panel: beside a shared screen, under your camera, or in a corner away from other attendees' video. Save each one once and jump between them with a shortcut:

1. Put the panel where it hides nothing. Use **Move** on the panel to snap it to an edge or corner, or drag it.
2. Press `Ctrl+Alt+Shift+1`. On an empty spot, the shortcut saves where the panel is now, together with its size and layout (full panel or mini bar).
3. Later, press `Ctrl+Alt+Shift+1` again to bring the panel back there.

**Move › Saved spots** on the panel and **Settings › General › Panel** show the spots and let you replace or forget one.

## Reach what is behind the panel

- `Ctrl+Alt+Shift+P` hides the panel for 6 seconds, long enough to click **Join** or a meeting control behind it. It comes back on its own.
- `Ctrl+Alt+Shift+T` lets clicks pass through the panel: it fades and the mouse reaches the window behind it. Press the shortcut again, or use **Let clicks pass through the panel** in the tray menu, to make the panel clickable again.

## Why these keys

The defaults avoid shortcuts that are already common on Windows laptops:

- meeting apps that use `Ctrl+Shift` combinations, such as mute in Teams;
- input methods that use `Ctrl+Space` or `Shift+Space`;
- graphics drivers that rotate the screen with `Ctrl+Alt` plus an arrow key, which is why moving the panel adds `Shift`;
- chat apps that use `Ctrl+Alt+A`, `Ctrl+Alt+W` or `Ctrl+Alt+Z`;
- function keys, which act as media keys on many laptops.

## Change a shortcut

1. Open **Settings › Shortcuts**.
2. Click **Change** next to the action.
3. Press the new key combination. Press `Esc` to cancel.

If another program already uses the combination, the row turns red with [MC-HK-001](/troubleshooting/error-codes#mc-hk-001). Choose a different combination. **Restore defaults** brings back the table above.

## When shortcuts don't reach MeetCue

Windows doesn't deliver shortcuts to MeetCue while the foreground window is a program running as administrator, or a game in exclusive full screen. See [Shortcuts and the panel](/troubleshooting/panel).
