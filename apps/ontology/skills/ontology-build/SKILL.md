---
name: ontology-build
description: >-
  Turn raw data of ANY format (CSV, TSV, Excel, JSON, JSON Lines, YAML, XML, Markdown,
  HTML, Word, PDF, plain text) into an RDF knowledge graph in the SenClaw Ontology app:
  ingest & profile the source, design the T-Box, author a declarative mapping, and lift
  rows into triples with provenance. Use when the user wants to BUILD an ontology /
  knowledge graph from data — e.g. "xây ontology từ file này", "bóc tách dữ liệu thành
  knowledge graph", "biến bảng này thành RDF", "map dữ liệu sang ontology", "build a
  knowledge graph from this data", "lift this spreadsheet to RDF". For querying /
  validating an EXISTING graph, use ontology-query instead.
---

# ontology-build

Build a knowledge graph from raw data in the **SenClaw Ontology** app via the
`ontology-mcp` server. The golden rule: **the schema (T-Box) is designed from competency
questions; the data (A-Box) is generated from the mapping.** Never let the column layout
dictate the ontology — that just produces a database schema in RDF clothing.

## When to use this skill

- The user has a file of any shape — spreadsheet, export, API dump, report, document —
  and wants it as RDF / a knowledge graph.
- The user wants to model a domain and load instance data into it.
- The user asks to "lift", "map", "bóc tách", or "triple-ify" data.

If the graph already exists and they want to query, validate, reason, or resolve
duplicates, use **ontology-query**.

## The fast path — use this first

For "here is my data, build me a graph", two calls do the whole thing:

1. `mcp__ontology-mcp__ontology_ingest` — one call per file. Paste the raw text as
   `content`; the format is auto-detected (CSV/TSV, JSON nested or wrapped, JSON Lines,
   YAML, XML, Markdown tables, HTML tables, prose). **Do not pre-convert anything** and do
   not set `kind`. One file may create several sources (one per sheet / table).
2. `mcp__ontology-mcp__ontology_autobuild` — runs profile → competency questions → T-Box →
   mapping (drafted, then mechanically checked against the real columns) → lift → extract
   from text → SHACL → competency suite → reasoning. It returns a `jobId`; poll
   `mcp__ontology-mcp__ontology_autobuild_status` until `done`.

Then read the result and **report it honestly**: the mapping `repairs` list says what the
draft got wrong, the validation report says what the data violates, and the competency
ratio says whether the ontology answers its own questions. Fix what matters with the
manual steps below rather than presenting a warn-laden build as a clean one.

Binary files (`.xlsx`, `.docx`, `.pdf`) cannot travel through MCP as text — tell the user
to drop them on the app's **Studio** tab, which posts the bytes to `/api/projects/:id/ingest`.

## Manual pipeline (when the user wants control over each stage)

1. **Create / pick a project.** `mcp__ontology-mcp__ontology_list_projects`, or
   `mcp__ontology-mcp__ontology_create_project` (`name`, optional `baseIri`). Keep the
   returned `id` — every other call needs `projectId`.
2. **Ingest & profile the source.** `mcp__ontology-mcp__ontology_ingest` with the file
   text as `content` and a short logical `name` (e.g. `products.csv`). The reply profiles
   each column: type, null ratio, uniqueness (candidate key), enum-ness, and a heuristic
   role (identifier / relation / attribute / enum), plus what the sniffer detected. Read it
   before designing anything. Nested structures arrive flattened with dotted paths
   (`customer.name`, `items.0.sku`) — those dotted names are what the mapping addresses.
3. **Design the T-Box.** Prefer `mcp__ontology-mcp__ontology_apply_tbox` with a full draft
   `{prefixes, classes, properties}`, or add terms one by one with
   `ontology_add_class` / `ontology_add_property`. Apply the four transforms:
   - **One row ≠ one entity.** A `orders(id, customer_name, product_sku, price)` row holds
     THREE entities — model Order, Customer, Product as separate classes.
   - **Denormalize → normalize.** A repeated `customer_name` is ONE Customer individual.
   - **Enum column → SKOS individuals, not classes.** `status="shipped"` →
     `:Shipped a :OrderStatus`, not a `:ShippedOrder` class.
   - **Relation with attributes → reification.** "Supplier X supplies Product Y at price Z
     from date D" needs an intermediate `:SupplyAgreement` class, not one object property.
4. **Author the mapping.** `mcp__ontology-mcp__ontology_set_mapping` with the RML-lite DSL
   (see below). Mint **stable IRIs**: use a natural key (`template`) when one exists, else
   `hash` the identifying columns — never a row number.
5. **Preview, then lift.** `mcp__ontology-mcp__ontology_preview_mapping` to sanity-check
   sample triples, then `mcp__ontology-mcp__ontology_lift` to materialize into a new
   provenance batch. Lifting is idempotent (RDF set semantics dedupes).
6. **Verify.** Run a couple of `mcp__ontology-mcp__ontology_sparql` queries and confirm the
   entity counts match the source. Tell the user it's open in the Ontology app.

## Mapping DSL (JSON)

```json
{
  "base": "http://senclaw.local/onto/shop",
  "prefixes": { "ex": "http://senclaw.local/onto/shop#" },
  "triplesMaps": [
    {
      "name": "ProductMap",
      "source": "products",
      "subject": { "template": "product/{sku}", "class": "ex:Product" },
      "predicateObjectMaps": [
        { "predicate": "rdfs:label", "object": { "column": "name" } },
        { "predicate": "ex:hasPrice", "object": { "column": "price", "datatype": "xsd:decimal" } },
        { "predicate": "ex:hasSupplier", "object": { "template": "supplier/{supplier_id}" } }
      ]
    },
    {
      "name": "SupplierMap",
      "source": "products",
      "subject": { "hash": ["supplier_name"], "seg": "supplier", "class": "ex:Supplier" },
      "predicateObjectMaps": [
        { "predicate": "rdfs:label", "object": { "column": "supplier_name" } }
      ]
    }
  ]
}
```

- **subject**: `{template, class}` with a natural key, or `{hash:[cols], seg, class}` for a
  keyless entity. A row whose key columns are empty is skipped.
- **object**: `{column, datatype?, lang?}` literal · `{template}` IRI referencing another
  entity by key · `{parentHash:[cols], parentSeg}` IRI for a keyless referenced entity ·
  `{constant, iri?}`.
- Two triples-maps over the SAME `source` is normal — that's how you split one row into
  several entities (Product + Supplier above).

## AI shortcuts

Every design step has an LLM draft you can offer the user (they review before applying):
`ontology_add_source` returns the profile; the app's `tbox/draft`, `mapping/draft`,
`shapes/draft` endpoints (and the ✨ buttons in the UI) draft a T-Box, mapping, or shapes
from the profile. When driving via MCP, prefer explicit `apply_tbox` / `set_mapping` calls
so the result is auditable.

## Notes

- Add the competency questions FIRST (via ontology-query / the Competency tab) if you know
  them — the AI T-Box draft uses them, and they become the acceptance test.
- Reply in the user's language (Vietnamese or English).
