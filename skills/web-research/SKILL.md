---
name: web-research
description: Multi-source web research with citation discipline and cross-verification. Use when a question needs an evidence-backed answer drawn from multiple live web sources (current data, comparisons, surveys, fact checks). Composes the `agent-browser` toolkit and adds research methodology — query planning, source diversity, claim verification, structured synthesis with URLs.
version: 1.0.0
when-to-use: questions that need a researched answer rather than a single fact ("compare X vs Y", "what's the current state of Z", "find studies on…", "is it true that…", "summarize today's news on…", multi-day market analyses, technical surveys, fact-checks). Prefer `agent-browser` directly for single-shot operations (open a page, screenshot, fill a form, one-snippet lookup like "giá vàng hôm nay").
---

# Web Research Skill

A methodology layer on top of `agent-browser`. Goal: produce an answer the user can **defend** — every claim has a source URL, important facts are cross-checked, contradictions are surfaced rather than smoothed over.

## When to use this vs `agent-browser` directly

| Request shape | Pick |
|---|---|
| "Open X / click Y / screenshot Z" | `agent-browser` (no methodology needed) |
| "Giá vàng hôm nay" (single fact) | `agent-browser` (one search, one snippet) |
| "Compare A vs B" | **`web-research`** (parallel queries per side) |
| "What do experts say about X" | **`web-research`** (multi-source) |
| "Summarize today's news on X" | **`web-research`** (multi-source) |
| "Is claim X true?" | **`web-research`** (adversarial verify) |
| "Survey of approaches to X" | **`web-research`** (deep mode) |

Default rule: if you'd write the answer as `X is true. Source: <one URL>`, use `agent-browser`. If you'd write `Most sources agree X; [source A][source B], but [source C] disagrees`, use `web-research`.

## Step 0 — Load browser tools (required)

This skill USES the `agent-browser` MCP toolkit; the tools are deferred and must be loaded first or the call fails with `InputValidationError`.

```
ToolSearch {
  "query": "select:mcp__browser__search,mcp__browser__navigate,mcp__browser__snapshot,mcp__browser__extract_text,mcp__browser__extract_structured,mcp__browser__extract_table,mcp__browser__screenshot,mcp__browser__close_tab",
  "max_results": 30
}
```

If a specific tool ends up needed (`crawl`, `extract_links`, etc.), load it the same way mid-task.

## Pick a research mode

The mode caps query count and depth. **Pick before you start** — over-research wastes user wall time; under-research produces shallow answers.

| Mode | When | Search queries | Pages fetched | Verification | Output |
|---|---|---|---|---|---|
| **Quick** | Single-fact-with-context (price, status, definition with source) | 1 | 0–1 | none | 1 paragraph + URL |
| **Medium** | Compare / explain / what-do-people-say | 3–5 distinct angles | 3–6 | corroborate the headline claim | TL;DR + body sections + 3–6 cited sources |
| **Deep** | Survey / fact-check / multi-day analysis | 8–15 (fan-out) | 8–15 | adversarial verify of every key claim | Structured report: TL;DR + sections + table of sources + gaps section |

Default = Medium. Promote to Deep only if the user explicitly asks for "deep research" or the question is high-stakes (a claim being made for the record, a financial decision, a published source). Demote to Quick if the user already has context.

## Workflow

### Step 1 — Restate the question + plan queries

Before any search, write down (internally / in the thought channel):

1. **Concrete question** (one sentence). Convert vague asks into a specific question.
2. **Key facets** to cover (2–5 bullets). Each facet → one or more queries.
3. **Source diversity targets**: news / docs / forums / official / opposing views. Note which apply.
4. **Recency requirement**: "as of today" vs "general" — affects query terms (add dates, "2026", "latest").

If the user is unclear, ASK 1–2 clarifying questions BEFORE researching — wrong premise produces wasted searches.

### Step 2 — Search broadly

Run 1 query per facet (parallel-friendly: emit multiple search calls in one turn if the tool harness supports it; otherwise serial).

```
mcp__browser__search {
  "query": "<facet-specific phrase>",
  "engine": "google",
  "num_results": 10,
  "language": "<locale of expected sources>"
}
```

**Query craft rules:**
- Add date/year for time-sensitive facts (`"latest 2026"`, `"hôm nay"`).
- Use the language of the likely sources (Vietnamese results need Vietnamese queries — `"giá vàng SJC"` not `"SJC gold price"`).
- For comparisons, run separate queries per side (`"X benefits"`, `"X drawbacks"`) — don't mix.
- For fact-checks, run a **refutation** query (`"X claim debunked"`, `"X false"`) alongside the supporting query.
- For surveys / "what do experts say", target distinct site types (`site:arxiv.org`, `site:reddit.com`, `site:hackernews.com`) in separate queries.

**Snippet triage** (cheap before fetching pages):
- Take the top 3–5 snippets per query.
- If snippets already answer the question with agreement across 3+ sources → skip page fetch, cite the sources, done.
- If snippets diverge or are too short → fetch the top 2–3 pages.

### Step 3 — Fetch + extract

For each page worth opening:

```
mcp__browser__navigate { "url": "..." }
↓
mcp__browser__extract_text { "tab_id": "<from navigate response>" }
```

Or for structured data:

```
mcp__browser__extract_structured {
  "tab_id": "...",
  "schema": { ... }
}
```

For tabular data use `mcp__browser__extract_table` (no schema needed).

**Close tabs** with `mcp__browser__close_tab` when done — keeps the user's browser tidy.

### Step 4 — Verify

For Medium / Deep, every claim that appears in the final answer must satisfy:

- **Corroboration**: at least 2 independent sources agree (independent = different domain, different author).
- **Recency check**: if the claim is time-sensitive, the source is dated within the requested window.
- **Refutation pass** (Deep only): run one query trying to refute the claim. If credible refutation exists, surface it as a "but…" rather than dropping it.
- **Quote sparingly**: prefer "Source X reports that…" framing over verbatim quotes (lets the user know it's reported, not a fact).

If sources contradict, do NOT pick one and hide the rest — present the disagreement with a one-line "why this might differ" (different methodology, different time window, opinion vs fact).

### Step 5 — Synthesize + cite

Output format depends on mode. See the **Output templates** section below.

Citation rules:
- Every factual sentence ends with `[Source: URL]` or `(SOURCE_NAME, URL)`.
- Group sources at the bottom only if the body has 5+ citations — otherwise inline.
- Use the **canonical URL** the user can open (no tracking suffixes, no `#anchor` unless it's the actual reference).
- If a fact comes from synthesis across multiple sources, cite all of them in a multi-source footnote, not just one.

### Step 6 — Report gaps (Deep only)

Deep research ends with a `## Gaps and uncertainties` section listing:
- Questions you couldn't answer with the available sources.
- Sources you'd want but couldn't access (paywall, behind login, dead links).
- Areas where the evidence is weak.

This is non-negotiable — pretending coverage was complete when it wasn't is the single worst failure mode.

## Output templates

### Quick mode (single paragraph + URL)

```
{Direct answer in 1–3 sentences}. [Source: {publisher}, {url}]
```

Example:

> Giá vàng SJC hôm nay là 75.5 triệu đồng mỗi lượng (mua) / 76.0 triệu (bán). [Source: PNJ, https://www.pnj.com.vn/...]

### Medium mode

```
**TL;DR**: {2–3 sentence summary including the key number / claim}.

**{Section 1}**
- {Point}. [Source: ...]
- {Point}. [Source: ...]

**{Section 2}**
- ...

**Sources**:
1. {Publisher} — {url}
2. {Publisher} — {url}
```

### Deep mode

```
# {Question}

**TL;DR**: {3–5 sentence summary}.

## {Major theme 1}

{Synthesized prose with inline citations.}

## {Major theme 2}

{Synthesized prose with inline citations.}

## Disagreements / contradictions

- {Source A says X, but Source B says Y because...}

## Sources

| # | Publisher | Date | URL | Relevance |
|---|---|---|---|---|
| 1 | ... | ... | ... | ... |

## Gaps and uncertainties

- {What we couldn't pin down}
```

## Decision tree

```
User asks a research question…
├── Is the question concrete?
│       NO → ask 1–2 clarifying questions, then continue
│       YES ↓
├── Pick mode (Quick / Medium / Deep)
├── Step 1: plan queries (facets, source types, recency)
├── Step 2: search (one query per facet)
├── Snippets answer it?
│       YES → cite snippets, done
│       NO ↓
├── Step 3: fetch top 2–5 pages, extract
├── Step 4: verify (corroborate, refute if Deep)
├── Conflicts found?
│       YES → surface them in the answer, don't hide
├── Step 5: synthesize + cite per template
└── Step 6 (Deep only): gaps section
```

## Rules

- **Plan before searching.** Don't fire queries reactively — they cost user wall time and pollute context.
- **Source diversity over quantity.** 3 distinct angles > 10 results from one site.
- **Cite everything.** No factual claim without `[Source: ...]` in Medium / Deep.
- **Surface disagreements.** If sources conflict, the user wants to know, not be protected from it.
- **Locale-aware queries.** Vietnamese question → Vietnamese query (otherwise you'll miss the best sources).
- **Recency-aware queries.** "Today" / "current" / "latest" → add date or "2026" / "hôm nay".
- **No fabrication.** If a fact isn't in a fetched source, don't include it. Better to say "I couldn't find this" than to invent.
- **Close tabs** after extracting — be a polite citizen of the user's browser.
- **Respect mode caps.** Quick = 1 search, Medium = 3–5, Deep ≤ 15. If you're hitting the cap and haven't answered, the question may need to be narrowed — ask the user.

## Failure handling

| Symptom | Cause | Fix |
|---|---|---|
| `InputValidationError` on `mcp__browser__*` | Tools not loaded | Run Step 0 `ToolSearch` first |
| Search returns 0 results | Bad query terms / engine rate-limit | Reword the query OR switch engine (`engine: "bing"`) |
| Snippets all from one site | Lopsided source coverage | Add `-site:dominant-domain.com` to next query, or specify `site:other-domain.com` |
| Conflicting sources | Real disagreement | Report the disagreement — don't pick one silently |
| Paywall / login required | Source not accessible | Note in Gaps section; find an alternative source |
| User question too vague | Premise unclear | STOP — ask 1–2 clarifying questions before searching |
| Found one source, can't corroborate | Insufficient corroboration | Mark the claim as "[Source X, uncorroborated]"; don't promote it to the TL;DR |
| Stale date on every source | Time-sensitive ask with old data | Run a query with stricter recency terms (`"2026"`, `"this week"`); if still old, say so in Gaps |

## Examples

### Quick: "What's Bitcoin price right now?"

```
1. ToolSearch { query: "select:mcp__browser__search" }
2. mcp__browser__search {
     "query": "bitcoin price USD live 2026",
     "num_results": 5
   }
3. Top 2 snippets agree → cite the higher-authority source (coinbase / coingecko)
4. Reply: "Bitcoin is currently ~$103,400 USD. [Source: CoinGecko, https://www.coingecko.com/en/coins/bitcoin]"
```

### Medium: "Compare Postgres vs MySQL for a startup"

```
1. ToolSearch { query: "select:mcp__browser__search,mcp__browser__navigate,mcp__browser__extract_text,mcp__browser__close_tab" }
2. Plan:
   - Facets: feature comparison, performance, ops, ecosystem, common gotchas
   - Source mix: official docs, recent benchmark, opinion from a startup eng
3. 4 parallel-style searches (one per facet)
4. Fetch top result per facet, extract text
5. Synthesize using Medium template:
   - TL;DR (2 sentences)
   - Section per facet, each with 2–3 cited points
   - Sources list at bottom (4 entries)
6. Close tabs
```

### Deep: "Has the AI safety field reached consensus on RLHF being a complete solution?"

```
1. ToolSearch (full toolkit incl. extract_structured)
2. Plan:
   - Facets: pro-RLHF position, criticisms, alternative approaches (Constitutional AI, DPO), recent papers, opinion polls
   - Sources: arxiv, anthropic/openai blogs, alignment forum, twitter from named researchers
   - Recency: "2025–2026 papers"
3. Fan-out: 10–12 queries with site: operators
4. Triage snippets; fetch ~8 pages
5. Verify by running 2 refutation queries ("RLHF limitations criticism")
6. Synthesize Deep template:
   - TL;DR
   - Sections: "Where consensus exists", "Where it doesn't", "Open problems", "Alternative approaches"
   - Disagreements section
   - Sources table (date-sorted)
   - Gaps: papers we couldn't access, areas with sparse evidence
```

## Composition with other skills

- After research → save key findings to wiki via the `wiki` skill (one entry per question for future reuse).
- Need to send the report to a chat / channel → use `bot-channels` to format for the destination.
- Research feeds a code change → pass key URLs as context to the coding skill, don't dump the full research transcript.

## See also

- `assets/templates.md` — query templates for common research patterns (price, news, comparison, fact-check, survey)
- `agent-browser` SKILL.md — underlying tool reference
