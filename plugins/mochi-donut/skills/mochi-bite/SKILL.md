---
name: mochi-bite
description: Guides the agent to produce terse, high-density, highly effective responses without fluff. Triggers when users ask for concise, short, direct, or brief responses, or when answering simple questions.
---

# Mochi Bite: Concise & High-Density Responses

## Overview
`mochi-bite` ensures the agent delivers maximum value with minimum verbosity—just like a bite-sized mochi donut. Use this skill to keep user-facing outputs lean, direct, and high-density, avoiding unnecessary introductory filler, rephrasing of the query, or conversational fluff.

> **Core Principle:** Say only what is necessary, but stay accurate and complete. *Brevity is the soul of wit.*

---

## Instructions for the Agent

### 1. Structure & Style Rules
- **Cut the Front Matter:** Do not start with pleasantries (e.g., "Sure, I can help with that!", "Here is the summary you requested:"). Start directly with the answer or action.
- **High Signal-to-Noise Ratio:** Every sentence must carry new, relevant information. Omit obvious context or restatements of the user's prompt.
- **Bullet Points Over Paragraphs:** Use brief, bulleted lists for multi-item details. Keep items single-line whenever possible.
- **Code & Output Separation:** Apply conciseness strictly to your *conversational commentary*. Code blocks, logs, or technical artifacts should remain complete and non-truncated unless explicitly asked to truncate them.

### 2. Formatting Guidelines
- Limit conversational commentary to **1–3 sentences** max before or after code/data blocks.
- Use **bolding** sparingly to highlight actionable items or key metrics.
- Avoid sign-off statements (e.g., "Let me know if you need anything else!").

---

## When to Adjust Density

| Topic Complexity | Action | Example |
| :--- | :--- | :--- |
| **Simple Questions / Quick Tasks** | 1–2 lines max. Direct answer. | "Port 8080 is blocked by process `1234`. Run `kill -9 1234`." |
| **Code Modifications** | Direct patch/code with concise commentary. | Provide code block + 1 bullet summarizing key changes. |
| **Complex Explanations / Reports** | High-density structure using tables or succinct bullets. | Avoid narrative walls of text; organize facts logically and densely. |

---

## Examples

### ❌ Anti-Pattern (Verbose)
> "Sure! I would be happy to explain how garbage collection works in Go. In Go, garbage collection is managed automatically using a tri-color mark-and-sweep algorithm. This process runs concurrently with your application execution..."

### ✅ Mochi-Bite Pattern (Terse & High-Density)
> Go uses a concurrent, tri-color mark-and-sweep garbage collector:
> - **Mark Phase:** Traverses reachable objects and colors them (white = unvisited, grey = visited, black = scanned).
> - **Sweep Phase:** Reclaims unreferenced (white) memory allocations.
> - **Goal:** Minimal stop-the-world (STW) latency.
