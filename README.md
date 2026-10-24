# MeetCue

**English** · [简体中文](README.zh-CN.md)

MeetCue is a desktop meeting prompter and interview practice tool. While you are in a meeting, it listens to the other participants (system sound) and to you (microphone), recognises questions addressed to you, and shows numbered talking points in a panel that stays on top without taking focus. As you answer, covered points are ticked and the next one is highlighted.

Practice mode replays subtitles from a past meeting or a list of interview questions with real-meeting timing, and gives you a review report afterwards.

MeetCue runs entirely on your computer. It calls the speech recognition and language model APIs you choose, with your own keys. There is no MeetCue server.

- **Documentation:** <https://amazingkenneth.github.io/meetcue/>
- **Install (Windows):** `scoop bucket add meetcue https://github.com/Amazingkenneth/meetcue` then `scoop install meetcue`
- **Try without keys:** run `MeetCue.exe --demo` to replay a sample interview.

## Features

- `Ctrl+Alt+Space` starts or pauses listening; the panel never steals focus and can be hidden from screen sharing.
- Talking points appear about 1.5 s after a question ends, with a full sample answer one click away.
- A question queue shows the previous, current and next question; interruptions wait their turn.
- A built-in latency probe measures each provider from your network and shows prices side by side, so you pick the combination yourself.
- Cost tracking per call, monthly usage, and optional budgets.
- English and Simplified Chinese interface, switchable at any time.

## Providers

| Task | Providers |
| --- | --- |
| Speech recognition | Soniox, Deepgram |
| Language models | Anthropic (Claude), any OpenAI-compatible API |

## Building from source

Requirements: Rust (stable), Node.js 22, and on Linux the WebKitGTK development packages.

```sh
npm ci
npm run dev:mock          # interface preview in a browser, no keys needed
cargo test --workspace --exclude meetcue-app
npm test
npx tauri dev             # run the desktop app (from app/src-tauri)
```

See [Development setup](https://amazingkenneth.github.io/meetcue/contribute/development) and [Architecture](https://amazingkenneth.github.io/meetcue/contribute/architecture).

## License

[MIT](LICENSE)
