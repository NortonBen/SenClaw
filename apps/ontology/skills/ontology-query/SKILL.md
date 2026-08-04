---
name: ontology-query
description: >-
  Query, validate, reason over, and clean an EXISTING RDF knowledge graph in the SenClaw
  Ontology app: run SPARQL, check competency questions as a test suite, validate with
  SHACL-lite, materialize RDFS/OWL-RL inferences, resolve duplicate entities, and extract
  triples from text. Use for "hỏi/truy vấn ontology bằng SPARQL", "kiểm tra competency
  question", "validate SHACL", "suy luận/materialize ontology", "gộp thực thể trùng",
  "trích triple từ văn bản", "query the ontology", "validate with SHACL", "reason over it".
  To BUILD a graph from raw data, use ontology-build.
---

# ontology-query

Interrogate and maintain a knowledge graph in the **SenClaw Ontology** app via the
`ontology-mcp` server. Assumes a project already has data lifted into it (see
ontology-build). Every call needs `projectId` — get it from
`mcp__ontology-mcp__ontology_list_projects`.

## Capabilities

### AIP Assist — questions about the platform or the project's metadata
`mcp__ontology-mcp__ontology_assist` (`projectId`, `question`, optional `context`). Use it
when the user asks **how something works** ("what does the mapping DSL support?", "why is
validation closed-world?") or **what is defined** ("which columns does the orders source
have?", "where did these triples come from?"). It retrieves over an index of platform
documentation plus this project's metadata and returns an answer with numbered citations.

Pass `context` — `{"tab": "...", "source": "..."}` with the tab the user is looking at
(`studio|sources|tbox|mapping|explore|competency|validate|governance`) and the source they
have open. It genuinely re-ranks retrieval, so the same question gets a better-targeted
answer; omitting it makes the tool measurably worse.

**Assist indexes metadata only — no cell values, no samples, no literals.** It therefore
cannot tell you what the data *says*. For counts, totals or values use `ontology_ask`;
Assist will tell the user the same thing rather than guess. Do not "help" by pasting its
citations into a numeric claim.

### Ask (start here for data questions)
`mcp__ontology-mcp__ontology_ask` (`projectId`, `question`) is the right tool whenever the
user asked a **question** rather than for a query. It grounds the translation on the
classes and predicates the data really contains, runs the query, repairs it once if it
fails or matches nothing, and returns a natural-language answer *plus* the SPARQL and the
rows. Always show the user the answer **and** mention the row count — the query is there so
the answer can be checked, not decoration.

Reach for `ontology_sparql` instead when the user asked for a query, wants an exact
aggregate you intend to write yourself, or when `ontology_ask` returned something you do
not believe.

### SPARQL
`mcp__ontology-mcp__ontology_sparql` (`projectId`, `query`). Standard prefixes
(rdf/rdfs/owl/xsd/skos/prov/sh) and the project's `ex:` are auto-declared. The default
graph is the **union of all data batches + inferred triples**, so plain `?s ?p ?o` sees
everything. Ground your query on `mcp__ontology-mcp__ontology_live_schema` — it lists the
classes and predicates that actually occur, with counts. The T-Box says what *should*
exist; the live schema says what *does*, and querying a declared-but-unused predicate is
the most common way to get a confidently empty answer.

### Competency questions (the acceptance test)
The ontology is "done" when each competency question is answered by one SPARQL query.
- `mcp__ontology-mcp__ontology_add_competency` (`question`, `sparql`, `expect`:
  `nonempty` | `empty` | `boolean`).
- `mcp__ontology-mcp__ontology_run_competency` runs them all and reports pass/fail.
Use these to prove the ontology actually answers the questions it was designed for.

### Validation (SHACL-lite, closed-world)
- `mcp__ontology-mcp__ontology_set_shapes` with
  `{nodeShapes:[{targetClass, properties:[{path, datatype?, class?, nodeKind?, minCount?,
  maxCount?, minInclusive?, maxInclusive?, pattern?}]}]}`.
- `mcp__ontology-mcp__ontology_validate` returns `{conforms, violations}`. SHACL is
  closed-world (missing data = violation) — the complement of OWL reasoning. Use it to
  gate data quality; don't expect it to infer anything.

### Reasoning (RDFS / OWL-RL subset, open-world)
`mcp__ontology-mcp__ontology_materialize` runs subclass / subproperty / domain / range /
inverse / `owl:sameAs`-symmetry rules to a fixpoint, into a dedicated inferred graph.
Open-world: it only ADDS facts. Run it before competency questions that rely on inference
(e.g. querying a superclass).

### Entity resolution
`mcp__ontology-mcp__ontology_resolve_candidates` (`projectId`, `class`, `labelProp?`,
`threshold?`) finds likely-duplicate individuals by Jaro-Winkler label similarity. Review
the pairs with the user, then link them (the UI applies `skos:closeMatch` by default —
safer than `owl:sameAs`, which is transitive and can contaminate a whole cluster).

### Provenance batches
`mcp__ontology-mcp__ontology_list_batches` lists each import (named graph) with live
counts; `mcp__ontology-mcp__ontology_drop_batch` (`iri`) removes exactly one lot — reload a
single source without disturbing the rest.

### Unstructured extraction
`mcp__ontology-mcp__ontology_extract` (`projectId`, `text`, optional `label`, `maxChunks`)
uses the LLM to pull triples from prose into a dedicated batch with provenance. Pass the
**whole document** — it is chunked internally, because the bridge returns a roughly fixed
amount of output per call and a single oversized prompt comes back summarized rather than
fully extracted. Treat the results as lower-confidence than mapped data; they live in their
own named graph so you can drop them if wrong.

### AIP Logic — LLM-proposed typed edits (human-in-the-loop)
When the user wants the LLM to **enrich or edit the graph** ("classify every product",
"extract the parties from these contracts", "tag each row"), use a Logic function — never
hand-write triples. The LLM emits **typed actions** validated against the T-Box; a
hallucinated class/property is rejected before it can become a triple.
- `mcp__ontology-mcp__ontology_create_function` (`name`, `kind`: extract|classify|resolve,
  `target`: source name for classify / class curie for resolve, `instruction`, `autoApply?`).
  Requires a T-Box to type-check against. **resolve** is deterministic (Jaro-Winkler label
  similarity, no LLM) and proposes skos:closeMatch links for duplicate individuals of a class —
  use it instead of `ontology_resolve_candidates`+`resolve/apply` when you want dedup to go
  through the reviewable proposal queue.
- `mcp__ontology-mcp__ontology_trial_function` — **always trial first**: preview the typed
  actions on a small sample with no writes.
- `mcp__ontology-mcp__ontology_run_function` — emit proposals into the queue.
- `mcp__ontology-mcp__ontology_list_proposals` / `ontology_approve_proposals` /
  `ontology_reject_proposals` — review and apply. Approving applies valid proposals as one
  provenance batch; **nothing touches the data until approved** (unless the function is
  autoApply). Report the proposals to the user and let them decide unless they said otherwise.

## Typical flows

- **"How many X are there / which Y has the most Z?"** → `ontology_ask` → report the
  answer, the count, and (if it matters) the query.
- **"What does this app/stage do?" / "What's in this project?"** → `ontology_assist` with
  the user's current tab as context → answer plus citations.
- **"Does my ontology answer X?"** → add the competency question + its SPARQL →
  `ontology_run_competency` → report pass/fail; if it relies on inference, `materialize`
  first.
- **"Is the data clean?"** → `ontology_set_shapes` → `ontology_validate` → summarize
  violations by constraint.
- **"Are there duplicate suppliers?"** → `ontology_resolve_candidates` on the class → show
  pairs → link on the user's confirmation.

Reply in the user's language (Vietnamese or English).
