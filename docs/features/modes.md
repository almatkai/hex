# Dictation rewriting with Antigravity

The [Modes flow](README.md#process-text-with-modes) selects a mode by website,
then application, else Global. Its rewrite runs after Global's and the mode's
corrections and before transformations, on the existing output worker on
macOS, Windows, and Linux. Capture never waits for the remote request.

```ts
Labs > Dictation > Antigravity rewriting         // modes.antigravity.opt-in
├── Off (default, Everyone) -> no Antigravity dictation requests; saved choices kept
└── On -> Modes joins the sidebar                 // modes.antigravity.configure
    └── Global or a custom mode > Processing
        ├── Rewrite provider: OpenCode | Antigravity
        ├── Rewrite dictation: off until enabled for this mode
        ├── Rewrite profile: Custom instructions | Punctuate | Translate to…
        │   ├── Punctuate -> capitalization, punctuation, spoken numbers to digits;
        │   │                keep language and wording, including Russian and Kazakh
        │   └── Translate to… -> English | Russian | Kazakh, saved per mode
        ├── Antigravity account -> Sign in with Google, Cancel, or Sign out
        │   └── Same credentials and account controls as Meetings; no Meetings lab required
        └── Model -> account's catalog, else the bundled Antigravity catalog

Rewrite starts                                  // modes.antigravity.run
├── Lab off -> skip rewrite; corrected transcript and transformations continue
├── Lab on -> native Antigravity generate over net; only this corrected transcript
│             and rewrite instructions, without app, site, title, or vocabulary context
├── Success -> trimmed answer, then transformations
└── Missing account, refusal, quota, empty answer, timeout, cancellation, or network error
    -> corrected transcript, then transformations; no dictated text in logs
```

The deadline (default 30 seconds) and cancellation include credential locking,
refresh, and all HTTP attempts. OpenCode and Antigravity keep independent model
choices. Old profiles deserialize as OpenCode with Custom instructions. Built-in
profiles replace the custom prompt; Punctuate and Translate do not skip
numbers-only transcripts. Output names the actual provider and omits the
Antigravity step while its lab is off. An Antigravity rewrite alone never asks
the browser for its page; website mode matching or a script transformation
still can. Its editor does not look for OpenCode. Turning the lab off while
Modes is open returns to Output.

Settings: `labs.antigravity_rewriting`; the `post_processing` object in
`dictation_processing.default_mode` and each entry of
`dictation_processing.modes`. These two JSON-valued `hex settings` keys cover
`provider`, `antigravity_model`, `style`, `target_language`, and the existing
`enabled`, `prompt`, `model`, `variant`, `deadline_seconds` fields. See
[CLI](../cli.md#antigravity-account).

Implementation: [processor](../../src/dictation/processor.rs),
[provider routing](../../src/dictation/processor/rewriting.rs),
[Antigravity](../../src/antigravity.rs),
[settings](../../src/app_settings/dictation_processing.rs), [Labs](../../src/labs.rs),
[rewrite controls](../../src/rune_ui/modes/rewriting.rs),
[account controls](../../src/rune_ui/antigravity.rs), and
[Output](../../src/rune_ui/output.rs).
The existing managed OpenCode-processing lock also disables the mode's rewrite.

Checks: processor tests cover provider/model routing, lab admission, corrections
before fallback, transformations after fallback, empty answers, and built-in
prompt rules. `antigravity/tests.rs` uses the existing local HTTP fixtures to
check the actual request body, quota fallback, stalled headers, and cancellation.
Settings tests cover old defaults and round-trips; i18n checks cover every new
string. These fixtures do not prove Russian or Kazakh model quality, physical
capture/paste, or a native window flow. Linux goldens have not been regenerated:
new scenes `modes-antigravity`, `modes-antigravity-signed-out`, and
`modes-antigravity-translate`; Labs scenes change their counts and newest cards.

Observed November 15, 2026 on Windows: locked build and clippy with warnings
denied passed. The verified Cargo-built test executable ran each filter with
isolated data: Antigravity 34 passed / 1 ignored, processor 27 / 2 ignored,
modes 19, Labs 67, i18n 25. Settings had 138 passed and the three existing
Russian-locale failures: `launch_publishes_the_saved_selection_before_the_worker_starts`,
`invalid_remembered_languages_are_not_offered_by_model_switching`, and
`model_switches_remember_language_without_changing_other_settings`.
All three new Modes scenes rendered and were inspected in light and dark on
Windows. One live punctuation attempt returned "Sign in to Antigravity again";
no model-quality result was obtained and it was not retried.

Pending Linux goldens, both appearances: those three new scenes plus
`mac-labs`, `mac-labs-link`, `mac-labs-on`, `mac-labs-category`,
`mac-labs-search`, `mac-labs-settings`, `mac-labs-screenshots`, `windows-labs`,
and `linux-labs`.
