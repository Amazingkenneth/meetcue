# Recognition and hint quality

This page helps when hints are missing, appear for the wrong sentences, or recognition gets words wrong.

## A question was asked but no hints appeared {#no-hints}

**Possible causes**

1. Listening is paused or the system sound level is flat.
2. **Only questions directed at me** is on and the question looked meant for someone else.
3. The question was very short or rhetorical ("right?", "okay?"); these are ignored on purpose.
4. Hint generation failed or timed out ([MC-LLM-001](/troubleshooting/error-codes#mc-llm-001), [MC-LLM-005](/troubleshooting/error-codes#mc-llm-005)).
5. The budget limit was reached and **Pause hint generation** is selected ([MC-BUD-002](/troubleshooting/error-codes#mc-bud-002)).

**Check**

1. Look at the status dot in the panel title bar: green means listening.
2. Open **Settings › Diagnostics**. Recent events show whether the sentence was recognised and how detection decided.

**Fix**

1. Start listening with `Ctrl+Alt+Space`; fix flat levels with [Audio and devices](/troubleshooting/audio).
2. Turn off **Only questions directed at me** if you want hints for every question.
3. For errors, follow the fix shown with the error code.

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).

## Hints appear for small talk {#too-many}

**Possible causes**

1. The detection model is too small for your meetings.
2. Several people ask each other questions and they aren't meant for you.

**Check**

1. Open the grey question text in the conversation to see why it was detected.

**Fix**

1. Turn on **Settings › Hints › Only questions directed at me**.
2. Fill in **My role** so detection knows what you are expected to answer.
3. Choose a stronger detection model in **Settings › Providers & cost**.

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).

## Names and terms are recognised wrongly {#recognition}

**Possible causes**

1. The meeting languages are set wrong.
2. Product names and acronyms are unknown to the recognition model.
3. The audio is quiet or noisy.

**Check**

1. Open **Settings › Audio › Meeting languages**.
2. Watch the transcript in the conversation while someone speaks.

**Fix**

1. Choose the languages actually spoken. Use **Chinese and English** when both appear in the same sentence.
2. Add an agenda or project notes as [briefing material](/guide/materials). Distinctive terms are passed to the recognition service as hints.
3. Try the other recognition provider: Soniox handles Chinese and English mixed in one sentence well.

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).

## One question is split into two, or hints answer only part of it {#split}

MeetCue joins the rest of a question to its start when the speaker pauses in the middle. Recognition ends a sentence at every pause, often with a full stop, so a sentence from the same speaker that begins within 5 seconds becomes part of the same question when nobody has answered in between and the earlier part did not end with a question mark ("如何制定数据中心IP。" / "V6的迁移方案，同时保证。" / "存量用户不受影响？"). A short tail that ends with a question mark ("…你会如何。" / "调整？") is joined too, and so is a sentence that opens with a joining word ("and…", "what about…", "还有…"). Short replies such as "对" or "OK" are never joined. The hints are rewritten to cover the whole question. A full sentence after a question mark may be a new question, so the detection model decides; once you have started answering, a new question always gets its own card.

When the speaker sets a question up over a few sentences before asking it ("如果研发和运维团队的职责重叠，交付很慢。我们发现了这些问题，那你会如何调整？"), the detection model tells MeetCue how many of those sentences belong to the question, and they are shown on the card with it.

**Possible causes**

1. The speaker paused for more than 5 seconds, or went on with words that don't sound like a continuation.
2. Recognition misheard the joining word.

**Check**

1. Look for two consecutive question blocks where the first one ends mid-sentence.

**Fix**

1. Increase **Settings › Audio › Advanced › Sentence end after silence**, for example from 700 to 1000 ms. Hints then appear slightly later.

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).
