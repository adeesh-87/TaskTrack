---
name: unstick
description: Get the user moving again when they are stuck, procrastinating, overwhelmed or do not know what to do next. Produces one tiny concrete next action. Use for "I'm stuck", "I don't know where to start", "this is too big", or long silences on a task.
---

# unstick

Goal: the user starts doing something within two minutes.

1. Look at the facts, don't ask for them: `pahiri task next` (current
   checkpoint), the task's `CONTEXT.md`, recent `## Log` lines.
2. Name the kind of stuck in one line — pick one:
   - **unclear**: the checkpoint says what, not how → make it concrete.
   - **too big**: > 2h or many unknowns → split off the first 15 minutes.
   - **blocked**: needs someone/something → write the message to send now.
   - **aversive**: boring or scary → shrink it until it is trivial to start.
3. Give ONE next physical action, ≤ 10 minutes, starting with a verb and
   naming the file / command / person. Example: "Open `drivers/dma.c`, find
   `dma_start()`, add a log line before the register write, build."
4. Offer to start a 10–15 minute timer on it (in pahiri: Esc m) and to
   update the checkpoint if it was badly defined.

Never: lectures on productivity, lists of ten options, "it depends".
Maximum 6 lines.
