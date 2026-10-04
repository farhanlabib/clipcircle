---
name: offload
description: Hand cheap, well-scoped work (explaining code, reviewing 1-3 files, small edits, drafting text) to cheaper models on the user's sub2api proxy via `.claude/tools/offload`, then validate the result, to save Claude tokens. Use proactively whenever such a task comes up and the user hasn't said to do it yourself.
---

# Offload to cheaper models

The user wants Claude tokens spent on judgment, not grunt work. Delegate, validate, and send it back if wrong.

## Commands (run with Bash from the repo root; only the final answer comes back)

```sh
.claude/tools/offload ask    -f path/A.kt -f path/B.kt "question"   # default deepseek/deepseek-v4.1-flash
.claude/tools/offload review -f path/A.kt ["focus"]                  # default xiaomi/mimo-v2.6-pro (slow, ~6 min, most careful)
.claude/tools/offload edit   "task with exact files and acceptance check"   # worktree; prints a .patch path
.claude/tools/offload models
```
`-m <model-id>` overrides. Proven alternatives: `z-ai/glm-5.3-flash` (second opinion), `deepseek/deepseek-v4.1-flash`.
Avoid for reviews: `xiaomi/mimo-v2.6-flash`, `meta/muse-spark-1.3-contributor` (said "no bugs" on a real bug).

## Good fits
- Explain a class or flow: `ask` with the files attached.
- Bug review of a small diff or 1-3 files: `review`. For important changes run a second model and compare.
- Small, specific edits in a known file (a fix whose location you already know): `edit`.
- Drafting release notes, store text, string resources, commit/PR descriptions: `ask`.

## Keep for yourself
Open-ended bug hunts, multi-file features, build/Gradle/Tauri changes, anything needing device testing, final review before a PR or release, anything with secrets.
Pick the files first: where the project has `graphify-out/graph.json`, run `graphify query "<question>"` (or `graphify explain`/`graphify path`) to find the relevant files cheaply, then pass them with `-f`. After applying an `edit` patch, run `graphify update .`.
Never ask the offload models to explore the repo freely: they wander and hit max turns. Always attach the files (`-f`) or name exact paths.

## Validate every result
- Check each claimed file:line and quote against the real code before repeating it to the user.
- For `edit`: read the patch, then `git apply <patch>` in the right branch, run the relevant tests yourself.
- If wrong, re-run with a correction ("Your claim X is wrong because Y; redo") instead of fixing it yourself, unless the fix is trivial.
- Tell the user which model did the work and what you verified.

## Failures
Exit code 75 = proxy/model unavailable (503/502/balance). Try one other model once; if still failing, do the task yourself and mention the proxy is down.

## Setup on a new Mac
Store the proxy key once in Terminal: `security add-generic-password -a "$USER" -s sub2api -w`. Requires the `claude` CLI and `python3`.

## Safety
The wrapper denies reads of local.properties, keystore/, *.jks, google-services.json, .env, state.json (device keys); `offload` refuses to inline secret-looking files. Do not paste secrets into prompts. Code goes to third-party providers via the user's proxy; the user approved this for this project's source.
