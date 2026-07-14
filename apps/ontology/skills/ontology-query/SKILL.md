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

### SPARQL
`mcp__ontology-mcp__ontology_sparql` (`projectId`, `query`). Standard prefixes
(rdf/rdfs/owl/xsd/skos/prov/sh) and the project's `ex:` are auto-declared. The default
graph is the **union of all data batches + inferred triples**, so plain `?s ?p ?o` sees
everything.

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
`mcp__ontology-mcp__ontology_extract` (`projectId`, `text`) uses the LLM to pull triples
from prose into a dedicated batch with provenance. Treat the results as lower-confidence
than mapped data — they live in their own named graph so you can drop them if wrong.

## Typical flows

- **"Does my ontology answer X?"** → add the competency question + its SPARQL →
  `ontology_run_competency` → report pass/fail; if it relies on inference, `materialize`
  first.
- **"Is the data clean?"** → `ontology_set_shapes` → `ontology_validate` → summarize
  violations by constraint.
- **"Are there duplicate suppliers?"** → `ontology_resolve_candidates` on the class → show
  pairs → link on the user's confirmation.

Reply in the user's language (Vietnamese or English).
