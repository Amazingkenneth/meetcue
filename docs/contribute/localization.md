# Localization

This page explains how MeetCue's English and Simplified Chinese text is organised, and how to keep both languages in step when you change something.

## Where text lives

| Text | Files |
| --- | --- |
| Interface, tray menu, notifications | `app/ui/locales/en.json`, `app/ui/locales/zh-CN.json` |
| Error titles and fixes | `crates/core/data/errors.json` (both languages in each entry) |
| Documentation | `docs/` for English, `docs/zh/` for Chinese, same file names |
| Model prompts | `crates/pipeline/prompts/*.txt` (English; the output language is a parameter) |

Code, comments and commit messages are in English.

## Change interface text

1. Add or change the key in **both** locale files in the same commit.
2. Translate whole sentences with placeholders, such as `"{{done}}/{{total}} covered"`. Never build a sentence from fragments; word order differs between languages.
3. For counts, use i18next plural keys (`_one`, `_other`) in English. Chinese only needs `_other`, but keep both keys so the files match.
4. Run `npm run check:locales`. It fails if a key exists in one file only or a value is empty.
5. Check the layout in both languages with `npm run dev:mock`. English is usually longer; leave room in buttons and table headers.

## Change error messages

Edit both `en` and `zh-CN` fields of the entry in `errors.json`. A fix is one sentence that tells the user what to do. The documentation's error code page is regenerated from this file during the docs build.

## Change documentation

1. Edit the English page and the page with the same path under `docs/zh/` in the same pull request.
2. Keep the same second-level headings in the same order. `docs/scripts/check-i18n.mjs` checks that every page exists in both languages with the same number of `##` headings.
3. When a link points at a heading, give the heading an explicit id such as `## Shortcuts do nothing {#shortcuts}` in both languages, so the anchor is the same.

## Writing style

- Start each page with one sentence that says what it helps with.
- Name headings after tasks or what the user sees: "Connect a microphone", not "Microphone".
- Number steps, one action per step.
- Use the exact button and menu names from the interface, in bold.
- Don't mention specific countries, cities or recommended regions. Endpoints are identified only by host name.
