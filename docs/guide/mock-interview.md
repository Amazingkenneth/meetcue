# Mock interview

This page shows how to test MeetCue from start to finish on your own, without hosting or joining a real meeting. MeetCue reads interview questions aloud, listens to your answers through your microphone, and shows talking points in the panel exactly as it would in a meeting. At the end you get a review report.

Allow about 15 minutes for your first mock interview, including setup.

## Three ways to test on your own

| Way | Needs keys | Uses your microphone | What it tests |
| --- | --- | --- | --- |
| [Demo replay](/quick-start) | No | No | How the panel looks and behaves, with a recorded interview |
| **Mock interview** (this page) | Yes | Yes | The whole app: speech recognition, question detection, hints, coverage ticks, latency and the review |
| [Meeting mode with a video](#meeting-video) | Yes | Yes | Meeting mode itself, with a recorded interview standing in for the other side |

Start with the demo if you haven't set up any keys yet. Use the mock interview for everything else.

## Before you start

1. **MeetCue is installed and set up.** You have finished [first-time setup](/setup), with a speech recognition key and a model key saved and verified. Without them, the mock interview can read questions but can't show hints.
2. **Wear headphones.** In a mock interview, MeetCue plays the questions through your output device and hears them through system sound, exactly as it hears other participants in a meeting. With speakers, your microphone picks the questions up too. MeetCue filters most of that echo, but headphones give the cleanest result.
3. **Check both level bars.** Open **Settings › Audio**, click **Test levels**, play any video and say something. The microphone bar should move when you speak and the system sound bar when the video plays. If one stays flat, see [Audio and devices](/troubleshooting/audio).
4. **Know the language.** The built-in software engineering and security samples are in English; the SDN transcript is in Chinese. By default, hints follow the language of the question; you can fix it under **Settings › General › Hint language**.

## Step 1: Open the mock interview window {#step-1}

You can open it from any of these places:

- **Tray menu**: click the MeetCue icon in the taskbar notification area and choose **Mock interview…** (third item from the top).
- **Panel**: while MeetCue isn't listening, the panel shows **Try a mock interview** under the start hint.
- **Settings › General › Mock interview**: click **Open mock interview**.
- **End of setup**: on the last step of the setup window, click **Finish and try a mock interview**.

The window opens with **How it works** at the top, followed by the sample interviews, your own file, and past practice.

The window guides you through four steps, shown at the top: **Sound check**, **Questions**, **Review** and **Practise**. Each step ends with a line saying what comes next.

## Step 2: Sound check and the asking voice {#step-2}

The first step shows both level bars live. Say something: the microphone bar should move. Play any video, or press **Try it** below: the system sound bar should move. If one stays flat, **Open audio settings** takes you to the device list; see [Audio and devices](/troubleshooting/audio).

Under **The voice that asks the questions**, pick the Windows voice for each language and press **Try it** to hear a sample sentence. **First voice Windows offers** keeps the old behaviour. Windows installs more voices under Settings › Time & language › Speech; a voice you add there appears here after restarting MeetCue. **Reading speed** is on the same step.

## Step 3: Tell MeetCue about the role (optional) {#step-3}

Hints are better when MeetCue knows who you are and what the interview is for. This step takes two minutes and is worth doing at least once.

1. In the sound check step, click **Open briefing material**. It opens **Settings › Hints**.
2. Under **My role**, write one sentence about yourself, for example:
   - `Backend software engineer with 4 years of Go and PostgreSQL`
   - `Network security engineer moving into AI security, 5 years in a SOC`
3. Under **Briefing material**, click **Add a file…** and choose a job description. Two samples match the built-in interviews:
   - [jd-software-engineer.md](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/materials/jd-software-engineer.md): backend software engineer
   - [jd-security-ai.md](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/materials/jd-security-ai.md): network security engineer on an AI platform team

   Download a sample with the **Download raw file** button on GitHub, or paste a real job description into a `.txt` or `.md` file.

See [Briefing material](/guide/materials) for what is sent and how to keep it focused. Remove the job description afterwards if you don't want it in your real meetings.

## Step 4: Choose an interview {#step-4}

Under **Try a sample interview**, click **Use this** next to one of the built-in interviews. You don't need to download anything.

| Sample | Kind | What happens |
| --- | --- | --- |
| Software engineer: question list | 14 questions | Fundamentals, algorithms, system design, debugging, databases, distributed systems and teamwork, one at a time |
| Software engineer: interview transcript | 6 answers | A backend engineer technical round. The interviewer's lines are read aloud; playback stops when it's your turn |
| Network security and AI: question list | 12 questions | TLS, zero trust, lateral movement, DNS exfiltration, ML for intrusion detection, prompt injection, RAG security, AI governance |
| Network security and AI: interview transcript | 5 answers | A security engineer round for an AI platform team |
| Network architecture: question list | 10 questions | Routing, data center fabrics and SDN |
| SDN interview transcript (Chinese) | 3 answers | A short interview in Chinese |

**Which one first?** Use a **question list** for your first run: MeetCue asks one question, waits for you, and moves on when you pause, which makes it easy to see each part working. Use a **transcript** to rehearse the flow of a real conversation, with small talk and follow-up questions in between.

Every sample has a reference answer for each question, so the review can tell you what you missed. The same files are in the repository under [samples/practice](https://github.com/Amazingkenneth/meetcue/tree/HEAD/samples/practice) if you want to edit them.

To use your own questions, click **Choose a file…** under **Use your own file**. See [Write your own interview](#own-interview).

## Step 5: Check the questions {#step-5}

The preview lists every question or line. You can:

- Edit a question's text by clicking it.
- Move questions up or down, or delete them.
- For a question list: **Shuffle**, or **Keep the first** few, for example 5 questions for a quick round.
- For a transcript: check **Which speaker is you?** MeetCue picks the speaker who asks the fewest questions, which is **Candidate** in the samples. Your lines aren't read aloud; they become the reference answers.

## Step 6: Choose how to practise {#step-6}

A mock interview works like a real meeting: every question is spoken aloud and comes to MeetCue as sound, and the first two switches decide whether your speech provider transcribes it. Leave both on to test the whole app; turn **Speech-to-text for the questions** off to prepare more cheaply.

For your first mock interview, use these settings:

| Option | First run | Why |
| --- | --- | --- |
| Speech-to-text for the questions | On | The spoken question is captured from system sound and transcribed by your speech provider, exactly like a meeting |
| Hint level | **Full** | Shows everything the panel can show |
| Pause that ends an answer | 3 seconds | Raise to 4 or 5 if MeetCue cuts you off while you think |
| Follow-up questions | On | Tests that MeetCue reacts to what you actually said |
| Speech-to-text for my answers | On | Needed for coverage ticks and most review figures |
| Stop reading when I start speaking | On | Lets you interrupt, as in a real conversation |

Then click **Start practice**. The panel comes to the front if it was hidden.

## Step 7: Answer out loud {#step-7}

For each question, this is what happens:

1. **MeetCue reads the question** in a Windows voice. The practice window shows *Reading the question*. Talking with the question still playing stops the reading.
2. **Hints appear in the panel** about a second and a half after the question ends: the question in grey and three to five numbered talking points, each with a bold keyword.
3. **You answer out loud**, as if the interviewer were listening. Use the points as cues, not as a script.
4. **Points tick off** as you cover them, and the next uncovered point is marked **Next**.
5. **A pause ends your answer.** After the pause you set (3 seconds by default), MeetCue sometimes asks a follow-up, then moves to the next question. If you don't start within 20 seconds, the panel shows a gentle reminder.

The practice window and the panel both show what is expected of you right now, and both have **Ask again**, which reads the current question from the start. Use it when you missed the question or want a second run at the same answer; the answer you had started is discarded and nothing extra is sent to the model.

MeetCue stops reading when you start to answer, but only for real speech: a cough, an "um" or a word misheard from the question itself doesn't cut the question short. You can turn interruption off entirely with **Stop reading when I start speaking** in the practice options.

These shortcuts work while another window has focus:

| Shortcut | Action |
| --- | --- |
| `Ctrl+Alt+Enter` | I've finished answering |
| `Ctrl+Alt+]` | Skip this question |
| `Ctrl+Alt+Space` | Pause or resume |
| `Ctrl+Alt+H` | Panel: full → mini bar → hidden |

The practice window has the same buttons, plus **End practice**.

## Step 8: Read the review {#step-8}

After the last question, or when you click **End practice**, the review opens in its own window. For each question and for the whole session it shows time to start, answer length, coverage, filler words, pace, and three short notes against the reference answer. See [Review report](/guide/practice#review-report) for what each figure means.

Click **Export Markdown** to save the report. Past sessions stay under **Past practice** in the mock interview window.

## A complete self-test plan {#self-test-plan}

Run these five rounds to cover everything MeetCue does, one feature at a time. Each round takes about five minutes.

| Round | Sample | Settings | Check that |
| --- | --- | --- | --- |
| 1 | Software engineer: question list, keep the first 5 | Speech-to-text on, Full | Each question is read, hints appear, points tick as you speak, the review opens |
| 2 | Network security and AI: question list, keep the first 5 | Speech-to-text on, Keywords only, with the security job description added | Hints mention the role (zero trust, detection, LLM security); only keywords show |
| 3 | Software engineer: interview transcript | Speech-to-text on, Full | Playback stops at your turns and waits; a greeting usually gets no hints; the review compares your answers with the candidate's lines |
| 4 | Network security and AI: interview transcript | Speech-to-text for the questions off, Delayed (5 s) | Hints appear about 5 seconds after each question, and the review notes that recognition delay was simulated |
| 5 | Any question list | Speech-to-text on, hints Off | No hints during the round, and you can still answer and get feedback in the review |

After round 1, look at the latency in the panel footer. Around 1.5 seconds is typical; much more points to a slow provider or network. See [High latency](/troubleshooting/latency).

## Test meeting mode with a video {#meeting-video}

A mock interview runs the same pipeline as a meeting, but its pacing is controlled by MeetCue. To test meeting mode itself, let a recording play the interviewer:

1. Put on headphones.
2. Open a recorded mock interview, for example a technical interview video on YouTube, and pause it at the start.
3. Press `Ctrl+Alt+Space` to start listening. The status dot turns green.
4. Play the video. When the interviewer asks a question, a question block with talking points appears.
5. Pause the video and answer out loud; watch the points tick off. Resume the video for the next question.
6. Press `Ctrl+Alt+Space` again to stop.

Anything played through your output device counts as the other side, so this also works with a recorded call or a friend's question read out by a text-to-speech website.

## Write your own interview {#own-interview}

The quickest format is a CSV file with a reference answer for each question. Only the `question` column is required:

```text
question,reference_answer,category,followups
"How does a hash map handle collisions?","Chaining or open addressing; resize when the load factor passes a threshold; a good hash function keeps chains short.",Data structures,"What is the worst-case lookup time?"
"How would you detect a compromised API key?","Alert on use from new locations or networks, unusual volume or endpoints; rotate keys regularly; scope keys with least privilege.",Security,
```

Separate several follow-ups with `|`. Question lists can also be plain text or Markdown with one question per line, and transcripts can be `.srt`, `.vtt` or `.txt`. All formats are described in [File formats](/reference/file-formats).

To turn a job description into a question list, you can ask any chat assistant:

```text
Write 12 technical interview questions for the job description below, as a CSV file
with the columns question, reference_answer, category and followups. Keep each
reference answer to two or three sentences. Separate follow-up questions with |.

<paste the job description>
```

Check the questions and answers before you practise with them.

## If something doesn't work {#troubleshooting}

| Symptom | Likely cause and fix |
| --- | --- |
| The question is read but no hints appear | A key or model problem. The panel shows an error code; click **See the fix**. Check the key under **Settings › Providers & cost** with **Verify** |
| No hints with speech-to-text for the questions on, but hints with it off | MeetCue can't hear system sound. Check the system sound bar and see [Audio and devices](/troubleshooting/audio) |
| Points never tick | **Speech-to-text for my answers** is off, or the microphone bar doesn't move. Check **Settings › Audio** |
| Points tick while the question is being read | The microphone hears the speakers. Use headphones, see [echo](/troubleshooting/audio#echo) |
| MeetCue moves on while you're still thinking | Raise **Pause that ends an answer** to 4 or 5 seconds |
| The voice reads too fast or too slowly | Change **Reading speed** in the sound check step |
| The question stops halfway | Something in the room interrupts it. Press **Ask again**, use headphones, and if it persists turn off **Stop reading when I start speaking** |
| The voice is wrong or unpleasant | Choose another one under **The voice that asks the questions** and press **Try it** |
| No voices are listed | Windows has no speech voice installed. Add one in Windows Settings › Time & language › Speech, then restart MeetCue |
| Hints are generic | Add **My role** and a job description, see [Step 2](#step-2) |

With both speech-to-text switches on, a mock interview uses your speech recognition and model providers just like a meeting, and costs about as much as a meeting of the same length. Turning off **Speech-to-text for the questions** skips recognition of the questions; the questions are still spoken aloud. Turning off **Speech-to-text for my answers** as well removes all recognition cost.
