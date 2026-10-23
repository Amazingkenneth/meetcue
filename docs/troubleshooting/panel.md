# Shortcuts and the panel

This page helps when shortcuts don't work, or the panel takes focus, disappears or shows up in screen sharing.

## Shortcuts do nothing {#shortcuts}

**Possible causes**

1. Another program registered the same combination first ([MC-HK-001](/troubleshooting/error-codes#mc-hk-001)).
2. The foreground window belongs to a program running as administrator.
3. A game or video player is in exclusive full screen.
4. MeetCue isn't running.

**Check**

1. Look for the MeetCue icon in the notification area of the taskbar.
2. Open **Settings › Shortcuts** and look for rows marked red.
3. Click on the desktop and try the shortcut again.

**Fix**

1. Change any red shortcut to a free combination.
2. Windows doesn't pass shortcuts from administrator windows to normal programs. Use the panel buttons while such a window is in front, or run the other program without administrator rights.
3. Switch the full-screen program to windowed or borderless mode.

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).

## Clicking the panel takes focus from the meeting {#focus}

**Possible causes**

1. A text field or menu in the panel needs keyboard focus.
2. The window manager of a remote desktop or virtual machine ignores the "don't activate" style.

**Check**

1. Click a talking point, then type in the meeting chat. The text should go to the chat.

**Fix**

1. Use the shortcuts instead of the mouse during a meeting.
2. Turn on **Settings › General › Let clicks pass through the panel**. The panel then ignores the mouse completely.

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).

## The panel is off screen or missing {#position}

**Possible causes**

1. It was on an external monitor that is no longer connected.
2. It is hidden; `Ctrl+Alt+H` cycles full → mini → hidden.

**Check**

1. Press `Ctrl+Alt+H` once or twice.

**Fix**

1. Right-click the tray icon and choose **Reset panel position**.

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).

## Others can see the panel when I share my screen {#screen-share}

**Possible causes**

1. **Hide the panel when sharing the screen** is off.
2. The recording method captures the display output directly. Some recorders, remote support tools and external capture devices do this.
3. Windows 10 earlier than version 2004 doesn't support hiding windows from capture.

**Check**

1. Open **Settings › General** and look at **Hide the panel when sharing the screen**.
2. Start screen sharing to yourself or a colleague, or take a screenshot with `Win+Shift+S`: the panel should be missing from the picture.

**Fix**

1. Turn the setting on.
2. Share a single window instead of the whole screen. The panel is never part of another app's window.
3. Use the mini bar or hide the panel with `Ctrl+Alt+H` when you can't rely on hiding.

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).
